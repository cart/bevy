use std::{alloc::Layout, cell::UnsafeCell, marker::PhantomData, ptr::NonNull};

#[derive(Debug)]
pub struct BlobVec {
    item_layout: Layout,
    capacity: usize,
    len: usize,
    data: UnsafeCell<NonNull<u8>>,
    swap_scratch: UnsafeCell<NonNull<u8>>,
    drop: unsafe fn(*mut u8),
}

impl BlobVec {
    pub fn new(item_layout: Layout, drop: unsafe fn(*mut u8), capacity: usize) -> BlobVec {
        // TODO: should this function be unsafe? should we check for non-zero layout sizes? what about unit structs?
        unsafe {
            let swap_scratch =
                UnsafeCell::new(NonNull::new(std::alloc::alloc(item_layout)).unwrap());
            let mut blob_vec = BlobVec {
                swap_scratch,
                item_layout,
                capacity: 0,
                len: 0,
                data: UnsafeCell::new(NonNull::dangling()),
                drop,
            };
            blob_vec.grow(capacity);
            blob_vec
        }
    }

    pub fn new_typed<T>(capacity: usize) -> BlobVec {
        BlobVec::new(Layout::new::<T>(), drop_ptr::<T>, capacity)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn grow(&mut self, increment: usize) {
        let new_capacity = self.capacity + increment;
        unsafe {
            let new_data = if self.capacity == 0 {
                std::alloc::alloc(Layout::from_size_align_unchecked(
                    self.item_layout.size() * new_capacity,
                    self.item_layout.align(),
                ))
            } else {
                std::alloc::realloc(
                    (*self.data.get()).as_ptr(),
                    Layout::from_size_align_unchecked(
                        self.item_layout.size() * self.capacity,
                        self.item_layout.align(),
                    ),
                    self.item_layout.size() * new_capacity,
                )
            };
            self.data = UnsafeCell::new(NonNull::new(new_data).unwrap());
        }
        self.capacity = new_capacity;
    }

    #[inline]
    pub unsafe fn set_unchecked(&self, index: usize, value: *mut u8) {
        let ptr = self.get_unchecked(index);
        std::ptr::copy_nonoverlapping(value, ptr, self.item_layout.size());
    }

    /// SAFETY: It is the caller's responsibility to ensure the value pointer points to data that matches self.item_layout
    /// This [BlobVec] will take ownership of `value`'s data, which means the caller should ensure that data is not dropped early  
    #[inline]
    pub unsafe fn push(&mut self, value: *mut u8) {
        if self.len == self.capacity {
            self.grow(self.capacity.max(1));
        }
        self.set_unchecked(self.len, value);
        self.len += 1;
    }

    /// SAFETY: It is the caller's responsibility to ensure the type matches self.item_layout
    #[inline]
    pub unsafe fn push_type<T>(&mut self, mut value: T) {
        self.push((&mut value as *mut T).cast::<u8>());
        std::mem::forget(value);
    }

    /// SAFETY: It is the caller's responsibility to ensure that `index` is < self.len()
    /// It is also the caller's responsibility to free the returned pointer.
    /// Callers should _only_ access the returned pointer immediately after calling this function.
    #[inline]
    pub unsafe fn swap_remove_and_forget_unchecked(&mut self, index: usize) -> *mut u8 {
        let last = self.len - 1;
        let swap_scratch = (*self.swap_scratch.get()).as_ptr();
        std::ptr::copy_nonoverlapping(
            self.get_unchecked(index),
            swap_scratch,
            self.item_layout.size(),
        );
        std::ptr::copy_nonoverlapping(
            self.get_unchecked(last),
            self.get_unchecked(index),
            self.item_layout.size(),
        );
        self.len -= 1;
        swap_scratch
    }

    pub unsafe fn swap_remove_type_unchecked<T>(&mut self, index: usize) -> T {
        let removed = self.swap_remove_and_forget_unchecked(index);
        std::ptr::read(removed.cast::<T>())
    }

    /// SAFETY: It is the caller's responsibility to ensure this isn't called when len() is 0
    /// This will _not_ call the drop function on the popped value, so it is the caller's responsibility to free
    /// that value at the appropriate time
    #[inline]
    pub unsafe fn pop_forget_unchecked(&mut self) {
        let ptr = self.get_unchecked(self.len - 1);
        self.len -= 1;
    }

    /// SAFETY: It is the caller's responsibility to ensure this isn't called when len() is 0
    #[inline]
    pub unsafe fn pop_unchecked(&mut self) {
        let ptr = self.get_unchecked(self.len - 1);
        self.len -= 1;
        (self.drop)(ptr);
    }

    /// SAFETY: It is the caller's responsibility to ensure the type matches self.item_layout
    // It is also the caller's responsibility to ensure this isn't called when len() is 0
    #[inline]
    pub unsafe fn pop_type_unchecked<T>(&mut self) -> T {
        let ptr = self.get_unchecked(self.len - 1);
        self.len -= 1;
        std::ptr::read(ptr.cast::<T>())
    }

    /// SAFETY: It is the caller's responsibility to ensure that `index` is < self.len()
    #[inline]
    pub unsafe fn get_unchecked(&self, index: usize) -> *mut u8 {
        (*self.data.get())
            .as_ptr()
            .add(index * self.item_layout.size())
            .cast::<u8>()
    }

    /// SAFETY: It is the caller's responsibility to ensure the type matches self.item_layout and that the type stored is T
    /// It is also the caller's responsibility to ensure that `index` is < self.len()
    #[inline]
    pub unsafe fn get_type_unchecked<T>(&self, index: usize) -> &T {
        &*self.get_unchecked(index).cast::<T>()
    }

    /// SAFETY: It is the caller's responsibility to ensure the type matches self.item_layout and that the type stored is T
    /// It is also the caller's responsibility to ensure that `index` is < self.len()
    #[inline]
    pub unsafe fn get_type_mut_unchecked<T>(&mut self, index: usize) -> &mut T {
        &mut *self.get_unchecked(index).cast::<T>()
    }

    pub fn clear(&mut self) {
        for i in 0..self.len {
            unsafe {
                let ptr = self.get_unchecked(i);
                (self.drop)(ptr);
            }
        }

        self.len = 0;
    }

    /// SAFETY: It is the caller's responsibility to ensure the type matches self.item_layout and that the type stored is T
    pub unsafe fn iter_type<T>(&self) -> BlobVecIter<'_, T> {
        BlobVecIter {
            value: self,
            index: 0,
            marker: Default::default(),
        }
    }

    /// SAFETY: It is the caller's responsibility to ensure the type matches self.item_layout and that the type stored is T
    pub unsafe fn iter_type_mut<T>(&mut self) -> BlobVecIterMut<'_, T> {
        BlobVecIterMut {
            value: self,
            index: 0,
            marker: Default::default(),
        }
    }
}

pub struct BlobVecIter<'a, T> {
    value: &'a BlobVec,
    index: usize,
    marker: PhantomData<T>,
}

impl<'a, T: 'static> Iterator for BlobVecIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.value.len {
            None
        } else {
            // SAFE: index is in-bounds and value is T (as determined by the caller of BlobVec::iter_type)
            unsafe {
                let value = self.value.get_type_unchecked::<T>(self.index);
                self.index += 1;
                Some(value)
            }
        }
    }
}

pub struct BlobVecIterMut<'a, T> {
    value: &'a mut BlobVec,
    index: usize,
    marker: PhantomData<T>,
}

impl<'a, T: 'static> Iterator for BlobVecIterMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.value.len {
            None
        } else {
            // SAFE: index is in-bounds and non-overlapping, value is T (as determined by the caller of BlobVec::iter_type)
            unsafe {
                let value = &mut *self.value.get_unchecked(self.index).cast::<T>();
                self.index += 1;
                Some(value)
            }
        }
    }
}

unsafe fn drop_ptr<T>(ptr: *mut u8) {
    ptr.cast::<T>().drop_in_place()
}

impl Drop for BlobVec {
    fn drop(&mut self) {
        if self.capacity > 0 {
            self.clear();
            unsafe {
                std::alloc::dealloc(
                    (*self.data.get()).as_ptr(),
                    Layout::from_size_align_unchecked(
                        self.item_layout.size() * self.capacity,
                        self.item_layout.align(),
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BlobVec;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Debug, Eq, PartialEq, Clone)]
    struct Foo {
        a: u8,
        b: String,
        drop_counter: Rc<RefCell<usize>>,
    }

    impl Drop for Foo {
        fn drop(&mut self) {
            *self.drop_counter.borrow_mut() += 1;
        }
    }

    #[test]
    fn blob_vec() {
        let drop_counter = Rc::new(RefCell::new(0));
        {
            let mut blob_vec = BlobVec::new_typed::<Foo>(2);
            assert_eq!(blob_vec.capacity(), 2);
            unsafe {
                let foo1 = Foo {
                    a: 42,
                    b: "abc".to_string(),
                    drop_counter: drop_counter.clone(),
                };
                blob_vec.push_type(foo1.clone());
                assert_eq!(blob_vec.len(), 1);
                assert_eq!(blob_vec.get_type_unchecked::<Foo>(0), &foo1);

                let mut foo2 = Foo {
                    a: 7,
                    b: "xyz".to_string(),
                    drop_counter: drop_counter.clone(),
                };
                blob_vec.push_type(foo2.clone());
                assert_eq!(blob_vec.len(), 2);
                assert_eq!(blob_vec.capacity(), 2);
                assert_eq!(blob_vec.get_type_unchecked::<Foo>(0), &foo1);
                assert_eq!(blob_vec.get_type_unchecked::<Foo>(1), &foo2);

                blob_vec.get_type_mut_unchecked::<Foo>(1).a += 1;
                assert_eq!(blob_vec.get_type_unchecked::<Foo>(1).a, 8);

                let foo3 = Foo {
                    a: 16,
                    b: "123".to_string(),
                    drop_counter: drop_counter.clone(),
                };

                blob_vec.push_type(foo3.clone());
                assert_eq!(blob_vec.len(), 3);
                assert_eq!(blob_vec.capacity(), 4);

                let value = blob_vec.pop_type_unchecked::<Foo>();
                assert_eq!(foo3, value);

                assert_eq!(blob_vec.len(), 2);
                assert_eq!(blob_vec.capacity(), 4);

                let value = blob_vec.swap_remove_type_unchecked::<Foo>(0);
                assert_eq!(foo1, value);
                assert_eq!(blob_vec.len(), 1);
                assert_eq!(blob_vec.capacity(), 4);

                foo2.a = 8;
                assert_eq!(blob_vec.get_type_unchecked::<Foo>(0), &foo2);
            }
        }

        assert_eq!(*drop_counter.borrow(), 6);
    }
}
