use std::ffi::{c_char, CStr};
use std::time::Instant;

#[repr(C)]
struct Pixel {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Samples {
    lanes: [i32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
union Word {
    code: i32,
    bits: u32,
}

#[link(name = "ffi_calls")]
extern "C" {
    fn fc_pixels(pixels: *const Pixel, count: i32) -> i64;
    fn fc_samples(samples: Samples) -> i64;
    fn fc_words(words: *mut Word, count: i32);
    fn fc_text_callback(count: extern "C" fn(*const c_char) -> i64) -> i64;
}

extern "C" fn character_count(text: *const c_char) -> i64 {
    let text = unsafe { CStr::from_ptr(text) }.to_str().unwrap();
    text.chars().count() as i64
}

fn measure(name: &str, expected: i64, mut call: impl FnMut(usize) -> i64) {
    let count = 2_000_000;
    let started = Instant::now();
    let total: i64 = (0..count).map(&mut call).sum();
    let elapsed = started.elapsed();
    assert_eq!(total, expected * count as i64);
    println!("{name} {}", elapsed.as_nanos() as f64 / count as f64);
}

fn main() {
    let pixels = [
        Pixel { x: 1, y: 2 },
        Pixel { x: 3, y: 4 },
        Pixel { x: 5, y: 6 },
        Pixel { x: 7, y: 8 },
    ];
    let samples = Samples {
        lanes: [1, 2, 3, 4],
    };
    let mut words = [Word { code: 0 }; 4];
    unsafe { fc_words(words.as_mut_ptr(), 4) };
    measure("struct_array", 36, |_| unsafe {
        fc_pixels(pixels.as_ptr(), 4)
    });
    measure("array_field", 10, |_| unsafe { fc_samples(samples) });
    measure("union_read", 4, |i| unsafe { words[i % 4].code as i64 });
    measure("text_callback", 4, |_| unsafe {
        fc_text_callback(character_count)
    });
}
