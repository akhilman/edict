use core::{any::TypeId, ptr::NonNull};

use crate::{archetype::Archetype, component::Component, epoch::Epoch};

use super::{Access, Fetch, ImmutableQuery, NonTrackingQuery, Query};

/// `Fetch` type for the `&T` query.
#[allow(missing_debug_implementations)]

pub struct FetchRead<T> {
    pub(super) ptr: NonNull<T>,
}

impl<'a, T> Fetch<'a> for FetchRead<T>
where
    T: Component,
{
    type Item = &'a T;

    #[inline]
    fn dangling() -> Self {
        FetchRead {
            ptr: NonNull::dangling(),
        }
    }

    #[inline]
    unsafe fn get_item(&mut self, idx: usize) -> &'a T {
        &*self.ptr.as_ptr().add(idx)
    }
}

unsafe impl<T> Query for &T
where
    T: Component,
{
    type Fetch = FetchRead<T>;

    #[inline]
    fn mutates() -> bool {
        false
    }

    #[inline]
    fn access(ty: TypeId) -> Access {
        if ty == TypeId::of::<T>() {
            Access::Shared
        } else {
            Access::None
        }
    }

    #[inline]
    fn allowed_with<Q: Query>() -> bool {
        matches!(Q::access(TypeId::of::<T>()), Access::None | Access::Shared)
    }

    #[inline]
    fn is_valid() -> bool {
        true
    }

    #[inline]
    fn skip_archetype(archetype: &Archetype, _: Epoch) -> bool {
        !archetype.contains_id(TypeId::of::<T>())
    }

    #[inline]
    unsafe fn fetch(archetype: &Archetype, _tracks: Epoch, _epoch: Epoch) -> Option<FetchRead<T>> {
        let idx = archetype.id_index(TypeId::of::<T>())?;
        let data = archetype.data(idx);
        debug_assert_eq!(data.id, TypeId::of::<T>());

        Some(FetchRead {
            ptr: data.ptr.cast(),
        })
    }
}

unsafe impl<T> ImmutableQuery for &T where T: Component {}
unsafe impl<T> NonTrackingQuery for &T where T: Component {}
