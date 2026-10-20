#![cfg(all(unix, any(not(target_os = "linux"), feature = "polling")))]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

use compio_driver::{DriverType, Proactor};

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static REQUESTED_BYTES: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

fn record(size: usize) {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
        REQUESTED_BYTES.with(|bytes| bytes.set(bytes.get() + size));
    }
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            record(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let ptr = unsafe { System.realloc(ptr, layout, size) };
        if !ptr.is_null() {
            record(size);
        }
        ptr
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn notified_polls_reuse_event_storage() {
    let mut driver = Proactor::builder()
        .driver_type(DriverType::Poll)
        .build()
        .unwrap();
    assert!(driver.driver_type().is_polling());
    let waker = driver.waker();
    for _ in 0..8 {
        waker.wake_by_ref();
        driver.poll(None).unwrap();
    }

    ALLOCATIONS.with(|count| count.set(0));
    REQUESTED_BYTES.with(|bytes| bytes.set(0));
    COUNTING.with(|counting| counting.set(true));
    for _ in 0..1000 {
        waker.wake_by_ref();
        driver.poll(None).unwrap();
    }
    COUNTING.with(|counting| counting.set(false));

    let allocations = ALLOCATIONS.with(Cell::get);
    let requested_bytes = REQUESTED_BYTES.with(Cell::get);
    assert_eq!(
        (allocations, requested_bytes),
        (0, 0),
        "warmed polling should reuse its event container"
    );
}
