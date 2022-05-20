//! Queries and iterators.
//!
//! To efficiently iterate over entities with specific set of components,
//! or only over thoses where specific component is modified, or missing,
//! [`Query`] is the solution.
//!
//! [`Query`] trait has a lot of implementations and is composable using tuples.

pub use self::{
    alt::{Alt, FetchAlt},
    filter::{Filter, With, Without},
    modified::{Modified, ModifiedFetchAlt, ModifiedFetchRead, ModifiedFetchWrite},
    read::FetchRead,
    write::FetchWrite,
};

use core::{any::TypeId, marker::PhantomData, ops::Range, ptr, slice};

use crate::{
    archetype::{chunk_idx, first_of_chunk, Archetype, CHUNK_LEN_USIZE},
    entity::EntityId,
};

mod alt;
mod filter;
mod modified;
mod option;
mod read;

#[cfg(feature = "rc")]
mod skip;
mod write;

pub use self::{alt::*, modified::*, option::*, read::*, write::*};

/// Trait implemented for `Query::Fetch` associated types.
pub trait Fetch<'a> {
    /// Item type this fetch type yields.
    type Item;

    /// Returns `Fetch` value that must not be used.
    fn dangling() -> Self;

    /// Checks if chunk with specified index must be skipped.
    unsafe fn skip_chunk(&self, chunk_idx: usize) -> bool;

    /// Checks if item with specified index must be skipped.
    unsafe fn skip_item(&self, idx: usize) -> bool;

    /// Notifies this fetch that it visits a chunk.
    unsafe fn visit_chunk(&mut self, chunk_idx: usize);

    /// Returns fetched item at specifeid index.
    unsafe fn get_item(&mut self, idx: usize) -> Self::Item;
}

/// Specifies kind of access query performs for particular component.
#[derive(Clone, Copy, Debug)]
pub enum Access {
    /// No access to component.
    /// Can be aliased with anything.
    None,

    /// Shared access to component. Can be aliased with other shared accesses.
    Shared,

    /// Cannot be aliased with other mutable and shared accesses.
    Mutable,
}

const fn merge_access(lhs: Access, rhs: Access) -> Access {
    match (lhs, rhs) {
        (Access::None, rhs) => rhs,
        (lhs, Access::None) => lhs,
        (Access::Shared, Access::Shared) => Access::Shared,
        _ => Access::Mutable,
    }
}

struct QueryAllowed<Q>(bool, PhantomData<Q>);

impl<L, R> core::ops::BitOr<QueryAllowed<L>> for QueryAllowed<R>
where
    L: Query,
    R: Query,
{
    type Output = QueryAllowed<(L, R)>;

    #[inline]
    fn bitor(self, rhs: QueryAllowed<L>) -> QueryAllowed<(L, R)> {
        let allowed = self.0 && rhs.0 && L::allowed_with::<R>();
        QueryAllowed(allowed, PhantomData)
    }
}

/// Trait for types that can query sets of components from entities in the world.
/// Queries implement efficient iteration over entities while yielding
/// sets of references to the components and optionally `EntityId` to address same components later.
pub unsafe trait Query {
    /// Fetch value type for this query type.
    /// Contains data from one archetype.
    type Fetch: for<'a> Fetch<'a>;

    /// Checks if this query type mutates any of the components.
    /// Queries that returns [`false`] must never attempt to modify a component.
    /// [`ImmutableQuery`] must statically return [`false`]
    /// and never attempt to modify a component.
    #[inline]
    fn mutates() -> bool {
        false
    }

    /// Checks if this query tracks changes of any of the components.
    #[inline]
    fn tracks() -> bool {
        false
    }

    /// Function to validate that query does not cause mutable reference aliasing.
    fn is_valid() -> bool;

    /// Returns what kind of access the query performs on the component type.
    fn access(ty: TypeId) -> Access;

    /// Returns `true` if query execution is allowed in parallel with specified.
    fn allowed_with<Q: Query>() -> bool;

    /// Checks if archetype must be skipped.
    fn skip_archetype(archetype: &Archetype, tracks: u64) -> bool;

    /// Fetches data from one archetype.
    /// Returns [`None`] is archetype does not match query requirements.
    unsafe fn fetch(archetype: &Archetype, tracks: u64, epoch: u64) -> Option<Self::Fetch>;
}

/// Query that does not mutate any components.
///
/// # Safety
///
/// `Query::mutate` must return `false`.
/// `Query` must not borrow components mutably.
/// `Query` must not change entities versions.
pub unsafe trait ImmutableQuery {}

/// Query that does not track component changes.
///
/// # Safety
///
/// `Query::tracks` must return `false`.
/// `Query` must not skip entities based on their versions.
pub unsafe trait NonTrackingQuery {}

/// Type alias for items returned by query type.
pub type QueryItem<'a, Q> = <<Q as Query>::Fetch as Fetch<'a>>::Item;

macro_rules! for_tuple {
    () => {
        for_tuple!(for A B C D E F G H I J K L M N O P);
    };

    (for) => {
        for_tuple!(impl);
    };

    (for $head:ident $($tail:ident)*) => {
        for_tuple!(for $($tail)*);
        for_tuple!(impl $head $($tail)*);
    };

    (impl) => {
        impl Fetch<'_> for () {
            type Item = ();

            #[inline]
            fn dangling() {}

            #[inline]
            unsafe fn skip_chunk(&self, _: usize) -> bool {
                false
            }

            #[inline]
            unsafe fn skip_item(&self, _: usize) -> bool {
                false
            }

            #[inline]
            unsafe fn visit_chunk(&mut self, _: usize) {}

            #[inline]
            unsafe fn get_item(&mut self, _: usize) {}
        }

        unsafe impl Query for () {
            type Fetch = ();

            #[inline]
            fn mutates() -> bool {
                false
            }

            #[inline]
            fn tracks() -> bool {
                false
            }

            #[inline]
            fn access(_ty: TypeId) -> Access {
                Access::None
            }

            #[inline]
            fn allowed_with<Q: Query>() -> bool {
                true
            }

            #[inline]
            fn is_valid() -> bool {
                true
            }

            #[inline]
            fn skip_archetype(_: &Archetype, _: u64) -> bool {
                false
            }

            #[inline]
            unsafe fn fetch(_: & Archetype, _: u64, _: u64) -> Option<()> {
                Some(())
            }
        }

        unsafe impl ImmutableQuery for () {}
        unsafe impl NonTrackingQuery for () {}

        impl Filter for () {}
    };

    (impl $($a:ident)+) => {
        impl<'a $(, $a)+> Fetch<'a> for ($($a,)+)
        where $($a: Fetch<'a>,)+
        {
            type Item = ($($a::Item,)+);

            #[inline]
            fn dangling() -> Self {
                ($($a::dangling(),)+)
            }

            #[inline]
            unsafe fn skip_chunk(&self, chunk_idx: usize) -> bool {
                #[allow(non_snake_case)]
                let ($($a,)+) = self;
                $($a.skip_chunk(chunk_idx) ||)+ false
            }

            /// Checks if item with specified index must be skipped.
            #[inline]
            unsafe fn skip_item(&self, idx: usize) -> bool {
                #[allow(non_snake_case)]
                let ($($a,)+) = self;
                $($a.skip_item(idx) ||)+ false
            }

            /// Notifies this fetch that it visits a chunk.
            #[inline]
            unsafe fn visit_chunk(&mut self, chunk_idx: usize) {
                #[allow(non_snake_case)]
                let ($($a,)+) = self;
                $($a.visit_chunk(chunk_idx);)+
            }

            #[inline]
            unsafe fn get_item(&mut self, idx: usize) -> ($($a::Item,)+) {
                #[allow(non_snake_case)]
                let ($($a,)+) = self;
                ($( $a.get_item(idx), )+)
            }
        }

        unsafe impl<$($a),+> Query for ($($a,)+) where $($a: Query,)+ {
            type Fetch = ($($a::Fetch,)+);

            #[inline]
            fn mutates() -> bool {
                false $( || $a::mutates()) +
            }

            #[inline]
            fn tracks() -> bool {
                false $( || $a::tracks()) +
            }

            #[inline]
            fn access(ty: TypeId) -> Access {
                let mut access = Access::None;
                $(access = merge_access(access, $a::access(ty));)+
                access
            }

            #[inline]
            fn allowed_with<Q: Query>() -> bool {
                $( <$a as Query>::allowed_with::<Q>() ) && +
            }

            #[inline]
            fn is_valid() -> bool {
                let allowed = $(
                    QueryAllowed(true, PhantomData::<$a>)
                ) | +;
                allowed.0
            }

            #[inline]
            fn skip_archetype(archetype: & Archetype, track: u64) -> bool {
                $( $a::skip_archetype(archetype, track) )||+
            }

            #[inline]
            unsafe fn fetch(archetype: & Archetype, track: u64, epoch: u64) -> Option<($($a::Fetch,)+)> {
                Some(($( $a::fetch(archetype, track, epoch)?, )+))
            }
        }

        unsafe impl<$($a),+> ImmutableQuery for ($($a,)+) where $($a: ImmutableQuery,)+ {}
        unsafe impl<$($a),+> NonTrackingQuery for ($($a,)+) where $($a: NonTrackingQuery,)+ {}

        impl<$($a),+> Filter for ($($a,)+) where $($a: Filter,)+ {
            #[inline]
            fn skip_archetype(&self, archetype: &Archetype, tracks: u64, epoch: u64) -> bool {
                #[allow(non_snake_case)]
                let ($($a,)+) = self;
                $( $a.skip_archetype(archetype, tracks, epoch) )||+
            }
        }
    };
}

for_tuple!();

/// Iterator over entities with a query `Q`.
/// Yields `EntityId` and query items for every matching entity.
///
/// Supports only `NonTrackingQuery`.
#[allow(missing_debug_implementations)]
pub struct QueryIter<'a, Q: Query, F = ()> {
    epoch: u64,
    archetypes: slice::Iter<'a, Archetype>,

    fetch: <Q as Query>::Fetch,
    entities: *const EntityId,
    indices: Range<usize>,

    filter: F,
}

impl<'a, Q, F> QueryIter<'a, Q, F>
where
    Q: Query,
{
    pub(crate) fn new(epoch: u64, archetypes: &'a [Archetype], filter: F) -> Self {
        QueryIter {
            epoch,
            archetypes: archetypes.iter(),
            fetch: Q::Fetch::dangling(),
            entities: ptr::null(),
            indices: 0..0,
            filter,
        }
    }
}

impl<'a, Q, F> Iterator for QueryIter<'a, Q, F>
where
    Q: Query,
    F: Filter,
{
    type Item = (EntityId, QueryItem<'a, Q>);

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }

    #[inline]
    fn next(&mut self) -> Option<(EntityId, QueryItem<'a, Q>)> {
        loop {
            match self.indices.next() {
                None => {
                    // move to the next archetype.
                    loop {
                        let archetype = self.archetypes.next()?;
                        if self.filter.skip_archetype(archetype, 0, self.epoch) {
                            continue;
                        }
                        if let Some(fetch) = unsafe { Q::fetch(archetype, 0, self.epoch) } {
                            self.fetch = fetch;
                            self.entities = archetype.entities().as_ptr();
                            self.indices = 0..archetype.len();
                            break;
                        }
                    }
                }
                Some(idx) => {
                    if Q::mutates() {
                        if let Some(chunk_idx) = first_of_chunk(idx) {
                            unsafe { self.fetch.visit_chunk(chunk_idx) }
                        }
                    }

                    debug_assert!(!unsafe { self.fetch.skip_item(idx) });

                    let item = unsafe { self.fetch.get_item(idx) };
                    let entity = unsafe { *self.entities.add(idx) };

                    return Some((entity, item));
                }
            }
        }
    }

    fn fold<B, Fun>(mut self, init: B, mut f: Fun) -> B
    where
        Self: Sized,
        Fun: FnMut(B, (EntityId, QueryItem<'a, Q>)) -> B,
    {
        let mut acc = init;
        for idx in self.indices {
            if Q::mutates() {
                if let Some(chunk_idx) = first_of_chunk(idx) {
                    unsafe { self.fetch.visit_chunk(chunk_idx) }
                }
            }
            debug_assert!(!unsafe { self.fetch.skip_item(idx) });

            let item = unsafe { self.fetch.get_item(idx) };
            let entity = unsafe { *self.entities.add(idx as usize) };

            acc = f(acc, (entity, item));
        }

        for archetype in self.archetypes {
            if self.filter.skip_archetype(archetype, 0, self.epoch) {
                continue;
            }
            if let Some(mut fetch) = unsafe { Q::fetch(archetype, 0, self.epoch) } {
                let entities = archetype.entities().as_ptr();

                for idx in 0..archetype.len() {
                    if Q::mutates() {
                        if let Some(chunk_idx) = first_of_chunk(idx) {
                            unsafe { fetch.visit_chunk(chunk_idx) }
                        }
                    }
                    debug_assert!(!unsafe { fetch.skip_item(idx) });

                    let item = unsafe { fetch.get_item(idx) };
                    let entity = unsafe { *entities.add(idx) };

                    acc = f(acc, (entity, item));
                }
            }
        }
        acc
    }
}

impl<Q, F> ExactSizeIterator for QueryIter<'_, Q, F>
where
    Q: Query,
    F: Filter,
{
    fn len(&self) -> usize {
        self.archetypes
            .clone()
            .fold(self.indices.len(), |acc, archetype| {
                if self.filter.skip_archetype(archetype, 0, self.epoch) {
                    return acc;
                }

                if Q::skip_archetype(archetype, 0) {
                    return acc;
                }

                acc + archetype.len()
            })
    }
}

/// Iterator over entities with a query `Q`.
/// Yields `EntityId` and query items for every matching entity.
///
/// Does not require `Q` to implement `NonTrackingQuery`.
#[allow(missing_debug_implementations)]
pub struct QueryTrackedIter<'a, Q: Query, F> {
    filter: F,
    tracks: u64,
    epoch: u64,
    archetypes: slice::Iter<'a, Archetype>,

    fetch: <Q as Query>::Fetch,
    entities: *const EntityId,
    indices: Range<usize>,
    visit_chunk: bool,
}

impl<'a, Q, F> QueryTrackedIter<'a, Q, F>
where
    Q: Query,
{
    pub(crate) fn new(tracks: u64, epoch: u64, archetypes: &'a [Archetype], filter: F) -> Self {
        QueryTrackedIter {
            filter,
            tracks,
            epoch,
            archetypes: archetypes.iter(),
            fetch: Q::Fetch::dangling(),
            entities: ptr::null(),
            indices: 0..0,
            visit_chunk: false,
        }
    }
}

impl<'a, Q, F> Iterator for QueryTrackedIter<'a, Q, F>
where
    Q: Query,
    F: Filter,
{
    type Item = (EntityId, QueryItem<'a, Q>);

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let upper = self
            .archetypes
            .clone()
            .fold(self.indices.len(), |acc, archetype| {
                if self.filter.skip_archetype(archetype, 0, self.epoch) {
                    return acc;
                }

                if Q::skip_archetype(archetype, 0) {
                    return acc;
                }

                acc + archetype.len()
            });

        (0, Some(upper))
    }

    #[inline]
    fn next(&mut self) -> Option<(EntityId, QueryItem<'a, Q>)> {
        loop {
            match self.indices.next() {
                None => {
                    // move to the next archetype.
                    loop {
                        let archetype = self.archetypes.next()?;

                        if self
                            .filter
                            .skip_archetype(archetype, self.tracks, self.epoch)
                        {
                            continue;
                        }

                        if let Some(fetch) = unsafe { Q::fetch(archetype, self.tracks, self.epoch) }
                        {
                            self.fetch = fetch;
                            self.entities = archetype.entities().as_ptr();
                            self.indices = 0..archetype.len();
                            break;
                        }
                    }
                }
                Some(idx) => {
                    if let Some(chunk_idx) = first_of_chunk(idx) {
                        if unsafe { self.fetch.skip_chunk(chunk_idx) } {
                            self.indices.nth(CHUNK_LEN_USIZE - 1);
                            continue;
                        }
                        self.visit_chunk = Q::mutates();
                    }

                    if !unsafe { self.fetch.skip_item(idx) } {
                        if self.visit_chunk {
                            unsafe { self.fetch.visit_chunk(chunk_idx(idx)) }
                            self.visit_chunk = false;
                        }

                        let item = unsafe { self.fetch.get_item(idx) };
                        let entity = unsafe { *self.entities.add(idx) };

                        return Some((entity, item));
                    }
                }
            }
        }
    }

    fn fold<B, Fun>(mut self, init: B, mut f: Fun) -> B
    where
        Self: Sized,
        Fun: FnMut(B, (EntityId, QueryItem<'a, Q>)) -> B,
    {
        let mut acc = init;
        while let Some(idx) = self.indices.next() {
            if let Some(chunk_idx) = first_of_chunk(idx) {
                if unsafe { self.fetch.skip_chunk(chunk_idx) } {
                    self.indices.nth(CHUNK_LEN_USIZE - 1);
                    continue;
                }
                self.visit_chunk = Q::mutates();
            }

            if !unsafe { self.fetch.skip_item(idx) } {
                if self.visit_chunk {
                    unsafe { self.fetch.visit_chunk(chunk_idx(idx)) }
                    self.visit_chunk = false;
                }
                let item = unsafe { self.fetch.get_item(idx) };
                let entity = unsafe { *self.entities.add(idx as usize) };

                acc = f(acc, (entity, item));
            }
        }

        for archetype in self.archetypes {
            if self
                .filter
                .skip_archetype(archetype, self.tracks, self.epoch)
            {
                continue;
            }
            if let Some(mut fetch) = unsafe { Q::fetch(archetype, self.tracks, self.epoch) } {
                let entities = archetype.entities().as_ptr();
                let mut indices = 0..archetype.len();

                while let Some(idx) = indices.next() {
                    if let Some(chunk_idx) = first_of_chunk(idx) {
                        if unsafe { fetch.skip_chunk(chunk_idx) } {
                            self.indices.nth(CHUNK_LEN_USIZE - 1);
                            continue;
                        }
                        self.visit_chunk = Q::mutates();
                    }

                    if !unsafe { fetch.skip_item(idx) } {
                        if self.visit_chunk {
                            unsafe { fetch.visit_chunk(chunk_idx(idx)) }
                            self.visit_chunk = false;
                        }
                        let item = unsafe { fetch.get_item(idx) };
                        let entity = unsafe { *entities.add(idx) };

                        acc = f(acc, (entity, item));
                    }
                }
            }
        }
        acc
    }
}
