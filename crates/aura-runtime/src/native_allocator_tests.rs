use super::*;

#[test]
fn a_freed_small_block_is_reused_for_the_same_size_class() {
    let allocator = AuraAllocator;
    let layout = Layout::from_size_align(24, 8).unwrap();
    let wider = Layout::from_size_align(32, 16).unwrap();
    // SAFETY: the test owns every block it makes and frees each once.
    unsafe {
        let first = allocator.alloc(layout);
        assert!(!first.is_null());
        first.write_bytes(0xa5, layout.size());
        allocator.dealloc(first, layout);
        let second = allocator.alloc_zeroed(wider);
        assert_eq!(first, second, "a block of the same size class is reused");
        assert!(std::slice::from_raw_parts(second, wider.size())
            .iter()
            .all(|byte| *byte == 0));
        allocator.dealloc(second, wider);
    }
}

#[test]
fn a_full_list_hands_half_its_blocks_to_the_shared_list() {
    let allocator = AuraAllocator;
    // A size no other test uses, made on a thread of its own.
    let layout = Layout::from_size_align(232, 8).unwrap();
    let class = class_of(layout).unwrap();
    std::thread::spawn(move || {
        // SAFETY: the thread owns every block it makes and frees each once.
        let blocks = (0..CAPACITY + 1)
            .map(|_| unsafe { allocator.alloc(layout) })
            .collect::<Vec<_>>();
        for block in &blocks {
            unsafe { allocator.dealloc(*block, layout) };
        }
        THREAD_BLOCKS
            .with(|lists| assert_eq!(lists.lists[class].len.get(), CAPACITY - CAPACITY / 2 + 1));
        // The shared half comes back once the thread's list runs dry.
        let again = (0..CAPACITY + 1)
            .map(|_| unsafe { allocator.alloc(layout) })
            .collect::<Vec<_>>();
        let mut sorted = again.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), again.len(), "no block is handed out twice");
        for block in again {
            unsafe { allocator.dealloc(block, layout) };
        }
    })
    .join()
    .unwrap();
}

#[test]
fn large_or_strictly_aligned_blocks_go_to_the_system_allocator() {
    let allocator = AuraAllocator;
    for layout in [
        Layout::from_size_align(MAX_SIZE + 1, 8).unwrap(),
        Layout::from_size_align(16, 32).unwrap(),
    ] {
        assert_eq!(class_of(layout), None);
        // SAFETY: the test owns the block.
        unsafe {
            let block = allocator.alloc_zeroed(layout);
            assert!(!block.is_null());
            assert_eq!(block as usize % layout.align(), 0);
            assert_eq!(*block, 0);
            allocator.dealloc(block, layout);
        }
    }
}

#[test]
fn realloc_keeps_the_contents_across_size_classes_and_the_system_allocator() {
    let allocator = AuraAllocator;
    let small = Layout::from_size_align(20, 4).unwrap();
    // SAFETY: the test owns the block through every move.
    unsafe {
        let block = allocator.alloc(small);
        for index in 0..20 {
            block.add(index).write(index as u8);
        }
        let same = allocator.realloc(block, small, 30);
        assert_eq!(same, block, "a block that stays in its class stays put");
        let grown = allocator.realloc(same, Layout::from_size_align(30, 4).unwrap(), 100);
        let large = allocator.realloc(grown, Layout::from_size_align(100, 4).unwrap(), 4000);
        let larger = allocator.realloc(large, Layout::from_size_align(4000, 4).unwrap(), 9000);
        let shrunk = allocator.realloc(larger, Layout::from_size_align(9000, 4).unwrap(), 10);
        assert_eq!(
            std::slice::from_raw_parts(shrunk, 10),
            &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
        allocator.dealloc(shrunk, Layout::from_size_align(10, 4).unwrap());
    }
}

#[test]
fn a_thread_that_ends_hands_its_blocks_and_chunk_to_the_shared_lists() {
    let allocator = AuraAllocator;
    let layout = Layout::from_size_align(1000, 8).unwrap();
    let class = class_of(layout).unwrap();
    let freed = std::thread::spawn(move || {
        // SAFETY: the thread makes and frees the block once.
        unsafe {
            let block = allocator.alloc(layout);
            allocator.dealloc(block, layout);
            block as usize
        }
    })
    .join()
    .unwrap();
    let mut shared = Vec::new();
    let mut batch = take_shared_batch(class);
    while !batch.is_null() {
        let mut block = batch;
        // SAFETY: the batches taken off the list are the test's now.
        unsafe {
            let next_batch = (*batch).next_batch;
            while !block.is_null() {
                shared.push(block as usize);
                block = (*block).next;
            }
            batch = next_batch;
        }
    }
    assert!(
        shared.contains(&freed),
        "the ended thread's block is shared"
    );
    for block in shared {
        // SAFETY: each block was free on the shared list.
        unsafe { share_batch(class, block as *mut FreeBlock, block as *mut FreeBlock) };
    }
}

#[test]
fn blocks_cut_from_chunks_do_not_overlap() {
    let allocator = AuraAllocator;
    std::thread::spawn(move || {
        let mut blocks = Vec::new();
        // Sizes above 960 B are left to the tests of the shared lists.
        for size in (1..=MAX_SIZE - 64)
            .step_by(7)
            .cycle()
            .take(4 * CHUNK / STEP)
        {
            let layout = Layout::from_size_align(size, 8).unwrap();
            // SAFETY: the thread owns the block.
            let block = unsafe { allocator.alloc(layout) };
            assert_eq!(block as usize % STEP, 0);
            blocks.push((
                block as usize,
                class_size(class_of(layout).unwrap()),
                layout,
            ));
        }
        let mut spans = blocks
            .iter()
            .map(|(at, len, _)| (*at, *len))
            .collect::<Vec<_>>();
        spans.sort();
        assert!(spans
            .windows(2)
            .all(|pair| pair[0].0 + pair[0].1 <= pair[1].0));
        for (block, _, layout) in blocks {
            // SAFETY: as above.
            unsafe { allocator.dealloc(block as *mut u8, layout) };
        }
    })
    .join()
    .unwrap();
}

#[test]
fn a_block_made_without_thread_lists_comes_from_the_shared_list_or_the_system() {
    let class = class_of(Layout::from_size_align(980, 8).unwrap()).unwrap();
    let first = unlisted_block(class);
    let second = unlisted_block(class);
    assert!(!first.is_null() && !second.is_null() && first != second);
    // SAFETY: the test owns both blocks and hands them over as one batch.
    unsafe {
        let (first, second) = (first.cast::<FreeBlock>(), second.cast::<FreeBlock>());
        (*first).next = second;
        share_batch(class, first, second);
    }
    // Other tests' threads may share blocks of the class meanwhile, so
    // take blocks until both come back.
    let mut taken = Vec::new();
    while !(taken.contains(&first) && taken.contains(&second)) && taken.len() < 64 {
        taken.push(unlisted_block(class));
    }
    assert!(taken.contains(&first) && taken.contains(&second));
    for block in taken {
        // SAFETY: each taken block is the test's.
        unsafe { share_batch(class, block.cast(), block.cast()) };
    }
}
