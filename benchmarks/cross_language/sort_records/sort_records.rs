//! Sorting: built-in sorts, key sorts, string sorts, and two hand-written sorts.
//!
//! Builds N employee records, then sorts the salaries with the built-in sort,
//! sorts the records by a composite integer key, sorts the names as text, runs a
//! hand-written quicksort (iterative, median of three, insertion sort for short
//! ranges) and a bottom-up merge sort, and finishes with binary searches over the
//! sorted salaries. Checksums over each result make every order visible.

const SYLLABLES: [&str; 16] = [
    "ka", "lo", "mi", "ra", "to", "ne", "su", "vi", "de", "an", "or", "el", "ix", "um", "po", "ze",
];
const MOD: i64 = 1000000007;

struct Rng {
    state: i64,
}

impl Rng {
    fn next(&mut self) -> i64 {
        self.state = self.state * 48271 % 2147483647;
        self.state
    }
}

#[derive(Clone)]
struct Record {
    id: i64,
    dept: i64,
    salary: i64,
    name: String,
}

fn record_key(record: &Record) -> i64 {
    // Department ascending, salary descending, id ascending.
    record.dept * 1000000000000 + (999999 - record.salary) * 1000000 + record.id
}

fn quicksort(xs: &mut [i64]) {
    let mut stack: Vec<i64> = vec![0, xs.len() as i64 - 1];
    while let Some(top) = stack.pop() {
        let mut hi = top;
        let mut lo = stack.pop().unwrap();
        while lo < hi {
            if hi - lo < 16 {
                for i in lo + 1..=hi {
                    let value = xs[i as usize];
                    let mut j = i - 1;
                    while j >= lo && xs[j as usize] > value {
                        xs[(j + 1) as usize] = xs[j as usize];
                        j -= 1;
                    }
                    xs[(j + 1) as usize] = value;
                }
                break;
            }
            let mid = (lo + hi) / 2;
            let (a, b, c) = (xs[lo as usize], xs[mid as usize], xs[hi as usize]);
            let pivot = if a < b {
                if b < c {
                    b
                } else if a < c {
                    c
                } else {
                    a
                }
            } else if a < c {
                a
            } else if b < c {
                c
            } else {
                b
            };
            let (mut i, mut j) = (lo, hi);
            while i <= j {
                while xs[i as usize] < pivot {
                    i += 1;
                }
                while xs[j as usize] > pivot {
                    j -= 1;
                }
                if i <= j {
                    xs.swap(i as usize, j as usize);
                    i += 1;
                    j -= 1;
                }
            }
            if j - lo < hi - i {
                stack.push(i);
                stack.push(hi);
                hi = j;
            } else {
                stack.push(lo);
                stack.push(j);
                lo = i;
            }
        }
    }
}

fn merge_sort(order: Vec<usize>, keys: &[i64]) -> Vec<usize> {
    let n = order.len();
    let mut source = order;
    let mut target = vec![0usize; n];
    let mut width = 1;
    while width < n {
        let mut start = 0;
        while start < n {
            let mid = (start + width).min(n);
            let end = (start + 2 * width).min(n);
            let (mut i, mut j, mut k) = (start, mid, start);
            while i < mid && j < end {
                if keys[source[j]] < keys[source[i]] {
                    target[k] = source[j];
                    j += 1;
                } else {
                    target[k] = source[i];
                    i += 1;
                }
                k += 1;
            }
            while i < mid {
                target[k] = source[i];
                i += 1;
                k += 1;
            }
            while j < end {
                target[k] = source[j];
                j += 1;
                k += 1;
            }
            start += 2 * width;
        }
        std::mem::swap(&mut source, &mut target);
        width *= 2;
    }
    source
}

fn lower_bound(xs: &[i64], value: i64) -> i64 {
    let (mut lo, mut hi) = (0, xs.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if xs[mid] < value {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo as i64
}

fn main() {
    let n: i64 = std::env::args().nth(1).unwrap().parse().unwrap();
    let mut rng = Rng { state: 41 };
    let mut records = Vec::new();
    for i in 0..n {
        let r = rng.next();
        let name = format!(
            "{}{}{}",
            SYLLABLES[(r % 16) as usize],
            SYLLABLES[((r / 16) % 16) as usize],
            SYLLABLES[((r / 256) % 16) as usize]
        );
        records.push(Record {
            id: i,
            dept: r % 20,
            salary: 30000 + (r / 20) % 170000,
            name,
        });
    }

    let salaries: Vec<i64> = records.iter().map(|record| record.salary).collect();
    let mut sorted_salaries = salaries.clone();
    sorted_salaries.sort();
    let mut salary_check = 0i64;
    for (i, &salary) in sorted_salaries.iter().enumerate() {
        salary_check = (salary_check * 31 + salary * (i as i64 % 7 + 1)) % MOD;
    }

    let mut by_key = records.clone();
    by_key.sort_by_key(record_key);
    let mut key_check = 0i64;
    for record in &by_key {
        key_check = (key_check * 31 + record.id) % MOD;
    }

    let mut names: Vec<String> = records.iter().map(|record| record.name.clone()).collect();
    names.sort();
    let mut distinct = 1;
    let mut name_check = 0i64;
    for i in 0..names.len() {
        if i > 0 && names[i] != names[i - 1] {
            distinct += 1;
        }
        name_check = (name_check * 31
            + names[i].len() as i64 * (i as i64 % 5 + 1)
            + names[i].as_bytes()[0] as i64)
            % MOD;
    }

    let mut quick = salaries.clone();
    quicksort(&mut quick);
    let quick_ok = quick == sorted_salaries;

    let order = merge_sort((0..n as usize).collect(), &salaries);
    let mut merge_check = 0i64;
    for &index in &order {
        merge_check = (merge_check * 31 + index as i64) % MOD;
    }

    let mut search_sum = 0;
    for _ in 0..n {
        search_sum += lower_bound(&sorted_salaries, 30000 + rng.next() % 170000);
    }

    println!(
        "sort_records n={n} salary_check={salary_check} key_check={key_check} distinct={distinct} \
         name_check={name_check} quick_ok={quick_ok} merge_check={merge_check} search_sum={search_sum}"
    );
}
