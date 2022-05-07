use core::{any::TypeId, cell::Cell, marker::PhantomData, ptr::NonNull};

use crate::{
    archetype::{chunk_idx, Archetype},
    component::Component,
    epoch::Epoch,
};

use super::{
    alt::{Alt, RefMut},
    Access, Fetch, ImmutableQuery, Query,
};

/// Query over modified component.
///
/// Should be used as either `Modified<&T>`, `Modified<&mut T>`
/// or `Modified<Alt<T>>`.
///
/// This is tracking query that requires providing subscriber's
/// `Tracks` to skip components that are not modified since the last time
/// that `Tracks` instance was used.
#[derive(Debug)]
pub struct Modified<T> {
    marker: PhantomData<fn() -> T>,
}

/// `Fetch` type for the `Modified<&T>` query.
#[allow(missing_debug_implementations)]
pub struct ModifiedFetchRead<T> {
    tracks: Epoch,
    ptr: NonNull<T>,
    entity_versions: NonNull<Epoch>,
    chunk_versions: NonNull<Epoch>,
}

impl<'a, T> Fetch<'a> for ModifiedFetchRead<T>
where
    T: 'a,
{
    type Item = &'a T;

    #[inline]
    fn dangling() -> Self {
        ModifiedFetchRead {
            tracks: 0,
            ptr: NonNull::dangling(),
            entity_versions: NonNull::dangling(),
            chunk_versions: NonNull::dangling(),
        }
    }

    #[inline]
    unsafe fn skip_chunk(&self, chunk_idx: usize) -> bool {
        let version = *self.chunk_versions.as_ptr().add(chunk_idx);
        version <= self.tracks
    }

    #[inline]
    unsafe fn skip_item(&self, idx: usize) -> bool {
        let version = *self.entity_versions.as_ptr().add(idx);
        version <= self.tracks
    }

    #[inline]
    unsafe fn get_item(&mut self, idx: usize) -> &'a T {
        &*self.ptr.as_ptr().add(idx)
    }
}

unsafe impl<T> Query for Modified<&T>
where
    T: Component,
{
    type Fetch = ModifiedFetchRead<T>;

    #[inline]
    fn mutates() -> bool {
        false
    }

    #[inline]
    fn tracks() -> bool {
        true
    }

    #[inline]
    fn access(ty: TypeId) -> Access {
        <&T as Query>::access(ty)
    }

    #[inline]
    fn allowed_with<Q: Query>() -> bool {
        <&T as Query>::allowed_with::<Q>()
    }

    #[inline]
    fn is_valid() -> bool {
        true
    }

    #[inline]
    fn skip_archetype(archetype: &Archetype, tracks: Epoch) -> bool {
        match archetype.id_index(TypeId::of::<T>()) {
            None => true,
            Some(idx) => unsafe {
                let data = archetype.data(idx);
                debug_assert_eq!(data.id, TypeId::of::<T>());
                *data.version.get() < tracks
            },
        }
    }

    #[inline]
    unsafe fn fetch(
        archetype: &Archetype,
        tracks: Epoch,
        _epoch: Epoch,
    ) -> Option<ModifiedFetchRead<T>> {
        let idx = archetype.id_index(TypeId::of::<T>())?;
        let data = archetype.data(idx);

        Some(ModifiedFetchRead {
            tracks,
            ptr: data.ptr.cast(),
            entity_versions: data.entity_versions,
            chunk_versions: data.chunk_versions,
        })
    }
}

unsafe impl<T> ImmutableQuery for Modified<&T> where T: Component {}

/// `Fetch` type for the `Modified<&mut T>` query.
#[allow(missing_debug_implementations)]
pub struct ModifiedFetchWrite<T> {
    tracks: Epoch,
    epoch: Epoch,
    ptr: NonNull<T>,
    entity_versions: NonNull<Epoch>,
    chunk_versions: NonNull<Epoch>,
}

impl<'a, T> Fetch<'a> for ModifiedFetchWrite<T>
where
    T: 'a,
{
    type Item = &'a mut T;

    #[inline]
    fn dangling() -> Self {
        ModifiedFetchWrite {
            tracks: 0,
            epoch: 0,
            ptr: NonNull::dangling(),
            entity_versions: NonNull::dangling(),
            chunk_versions: NonNull::dangling(),
        }
    }

    #[inline]
    unsafe fn skip_chunk(&self, chunk_idx: usize) -> bool {
        let version = *self.chunk_versions.as_ptr().add(chunk_idx);
        version <= self.tracks
    }

    #[inline]
    unsafe fn skip_item(&self, idx: usize) -> bool {
        let version = *self.entity_versions.as_ptr().add(idx);
        version <= self.tracks
    }

    #[inline]
    unsafe fn visit_chunk(&mut self, chunk_idx: usize) {
        let chunk_version = &mut *self.chunk_versions.as_ptr().add(chunk_idx);

        debug_assert!(*chunk_version < self.epoch);
        *chunk_version = self.epoch;
    }

    #[inline]
    unsafe fn get_item(&mut self, idx: usize) -> &'a mut T {
        let entity_version = &mut *self.entity_versions.as_ptr().add(idx);

        debug_assert!(*entity_version < self.epoch);
        *entity_version = self.epoch;

        &mut *self.ptr.as_ptr().add(idx)
    }
}

unsafe impl<T> Query for Modified<&mut T>
where
    T: Component,
{
    type Fetch = ModifiedFetchWrite<T>;

    #[inline]
    fn mutates() -> bool {
        true
    }

    #[inline]
    fn tracks() -> bool {
        true
    }

    #[inline]
    fn access(ty: TypeId) -> Access {
        <&mut T as Query>::access(ty)
    }

    #[inline]
    fn allowed_with<Q: Query>() -> bool {
        <&mut T as Query>::allowed_with::<Q>()
    }

    #[inline]
    fn is_valid() -> bool {
        true
    }

    #[inline]
    fn skip_archetype(archetype: &Archetype, tracks: Epoch) -> bool {
        match archetype.id_index(TypeId::of::<T>()) {
            None => true,
            Some(idx) => unsafe {
                let data = archetype.data(idx);
                debug_assert_eq!(data.id, TypeId::of::<T>());
                *data.version.get() < tracks
            },
        }
    }

    #[inline]
    unsafe fn fetch(
        archetype: &Archetype,
        tracks: Epoch,
        epoch: Epoch,
    ) -> Option<ModifiedFetchWrite<T>> {
        let idx = archetype.id_index(TypeId::of::<T>())?;
        let data = archetype.data(idx);

        debug_assert!(*data.version.get() < epoch);
        *data.version.get() = epoch;

        Some(ModifiedFetchWrite {
            tracks,
            epoch,
            ptr: data.ptr.cast(),
            entity_versions: data.entity_versions,
            chunk_versions: data.chunk_versions,
        })
    }
}

#[allow(missing_debug_implementations)]
/// `Fetch` type for the `Modified<Alt<T>>` query.
pub struct ModifiedFetchAlt<T> {
    tracks: Epoch,
    epoch: Epoch,
    ptr: NonNull<T>,
    entity_versions: NonNull<Epoch>,
    chunk_versions: NonNull<Cell<Epoch>>,
}

impl<'a, T> Fetch<'a> for ModifiedFetchAlt<T>
where
    T: 'a,
{
    type Item = RefMut<'a, T>;

    #[inline]
    fn dangling() -> Self {
        ModifiedFetchAlt {
            tracks: 0,
            epoch: 0,
            ptr: NonNull::dangling(),
            entity_versions: NonNull::dangling(),
            chunk_versions: NonNull::dangling(),
        }
    }

    #[inline]
    unsafe fn skip_chunk(&self, chunk_idx: usize) -> bool {
        let version = &*self.chunk_versions.as_ptr().add(chunk_idx);
        version.get() <= self.tracks
    }

    #[inline]
    unsafe fn skip_item(&self, idx: usize) -> bool {
        let version = *self.entity_versions.as_ptr().add(idx);
        version <= self.tracks
    }

    #[inline]
    unsafe fn get_item(&mut self, idx: usize) -> RefMut<'a, T> {
        RefMut {
            component: &mut *self.ptr.as_ptr().add(idx),
            entity_version: &mut *self.entity_versions.as_ptr().add(idx),
            chunk_version: &*self.chunk_versions.as_ptr().add(chunk_idx(idx)),
            epoch: self.epoch,
        }
    }
}

unsafe impl<T> Query for Modified<Alt<T>>
where
    T: Component,
{
    type Fetch = ModifiedFetchAlt<T>;

    #[inline]
    fn mutates() -> bool {
        true
    }

    #[inline]
    fn tracks() -> bool {
        true
    }

    #[inline]
    fn access(ty: TypeId) -> Access {
        <Alt<T> as Query>::access(ty)
    }

    #[inline]
    fn allowed_with<Q: Query>() -> bool {
        <Alt<T> as Query>::allowed_with::<Q>()
    }

    #[inline]
    fn is_valid() -> bool {
        true
    }

    #[inline]
    fn skip_archetype(archetype: &Archetype, tracks: Epoch) -> bool {
        match archetype.id_index(TypeId::of::<T>()) {
            None => true,
            Some(idx) => unsafe {
                let data = archetype.data(idx);
                debug_assert_eq!(data.id, TypeId::of::<T>());
                *data.version.get() < tracks
            },
        }
    }

    #[inline]
    unsafe fn fetch(
        archetype: &Archetype,
        tracks: Epoch,
        epoch: Epoch,
    ) -> Option<ModifiedFetchAlt<T>> {
        let idx = archetype.id_index(TypeId::of::<T>())?;
        let data = archetype.data(idx);
        debug_assert_eq!(data.id, TypeId::of::<T>());

        if *data.version.get() < tracks {
            return None;
        }

        debug_assert!(*data.version.get() < epoch);
        *data.version.get() = epoch;

        Some(ModifiedFetchAlt {
            tracks,
            epoch,
            ptr: data.ptr.cast(),
            entity_versions: data.entity_versions,
            chunk_versions: data.chunk_versions.cast(),
        })
    }
}
