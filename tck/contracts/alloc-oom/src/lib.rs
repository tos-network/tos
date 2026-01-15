//! Allocation OOM test contract
//!
//! Attempts to exhaust the VM heap and confirms allocation returns null
//! instead of panicking.

#![no_std]
#![no_main]

extern crate alloc;

use alloc::alloc::alloc;
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use tako_sdk::{get_heap_region, log};

struct VmBumpAllocator;

static HEAP_START: AtomicUsize = AtomicUsize::new(0);
static HEAP_END: AtomicUsize = AtomicUsize::new(0);
static HEAP_POS: AtomicUsize = AtomicUsize::new(0);

fn init_heap() {
    if HEAP_START.load(Ordering::Relaxed) != 0 {
        return;
    }

    let (heap_start, heap_size) = get_heap_region();
    let start = heap_start as usize;
    let end = start.saturating_add(heap_size as usize);

    HEAP_START.store(start, Ordering::Relaxed);
    HEAP_END.store(end, Ordering::Relaxed);
    HEAP_POS.store(start, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for VmBumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        init_heap();

        let align = layout.align();
        let size = layout.size();
        let current = HEAP_POS.load(Ordering::Relaxed);
        let aligned = (current + align - 1) & !(align - 1);
        let next = aligned.saturating_add(size);
        let heap_end = HEAP_END.load(Ordering::Relaxed);

        if next > heap_end {
            core::ptr::null_mut()
        } else {
            HEAP_POS.store(next, Ordering::Relaxed);
            aligned as *mut u8
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // no-op bump allocator
    }
}

#[global_allocator]
static ALLOCATOR: VmBumpAllocator = VmBumpAllocator;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn entrypoint() -> u64 {
    log("=== Alloc OOM Test ===");

    let mut allocations: u64 = 0;
    let mut size: usize = 4 * 1024;

    for _ in 0..128 {
        let layout = match Layout::from_size_align(size, 8) {
            Ok(layout) => layout,
            Err(_) => {
                log("Invalid layout");
                return 2;
            }
        };

        let ptr = unsafe { alloc(layout) };
        if ptr.is_null() {
            log("OOM reached: allocation returned null");
            log("OOM handled");
            return 0;
        }

        allocations += 1;
        unsafe {
            core::ptr::write_bytes(ptr, 0xA5, 1);
        }

        size = size.saturating_add(1024);
    }

    log("OOM not reached within allocation limit");
    log("allocations exceeded threshold");
    1
}
