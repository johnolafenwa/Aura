"""Sorting: built-in sorts, key sorts, string sorts, and two hand-written sorts.

Builds N employee records, then sorts the salaries with the built-in sort,
sorts the records by a composite integer key, sorts the names as text, runs a
hand-written quicksort (iterative, median of three, insertion sort for short
ranges) and a bottom-up merge sort, and finishes with binary searches over the
sorted salaries. Checksums over each result make every order visible.
"""
import sys

SYLLABLES = ["ka", "lo", "mi", "ra", "to", "ne", "su", "vi", "de", "an", "or", "el", "ix", "um", "po", "ze"]
MOD = 1000000007


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


class Record:
    def __init__(self, id, dept, salary, name):
        self.id = id
        self.dept = dept
        self.salary = salary
        self.name = name


def record_key(record):
    # Department ascending, salary descending, id ascending.
    return record.dept * 1000000000000 + (999999 - record.salary) * 1000000 + record.id


def quicksort(xs):
    stack = [0, len(xs) - 1]
    while stack:
        hi = stack.pop()
        lo = stack.pop()
        while lo < hi:
            if hi - lo < 16:
                for i in range(lo + 1, hi + 1):
                    value = xs[i]
                    j = i - 1
                    while j >= lo and xs[j] > value:
                        xs[j + 1] = xs[j]
                        j -= 1
                    xs[j + 1] = value
                break
            mid = (lo + hi) // 2
            a = xs[lo]
            b = xs[mid]
            c = xs[hi]
            if a < b:
                pivot = b if b < c else (c if a < c else a)
            else:
                pivot = a if a < c else (c if b < c else b)
            i = lo
            j = hi
            while i <= j:
                while xs[i] < pivot:
                    i += 1
                while xs[j] > pivot:
                    j -= 1
                if i <= j:
                    xs[i], xs[j] = xs[j], xs[i]
                    i += 1
                    j -= 1
            if j - lo < hi - i:
                stack.append(i)
                stack.append(hi)
                hi = j
            else:
                stack.append(lo)
                stack.append(j)
                lo = i


def merge_sort(order, keys):
    n = len(order)
    source = order
    target = [0] * n
    width = 1
    while width < n:
        start = 0
        while start < n:
            mid = min(start + width, n)
            end = min(start + 2 * width, n)
            i = start
            j = mid
            k = start
            while i < mid and j < end:
                if keys[source[j]] < keys[source[i]]:
                    target[k] = source[j]
                    j += 1
                else:
                    target[k] = source[i]
                    i += 1
                k += 1
            while i < mid:
                target[k] = source[i]
                i += 1
                k += 1
            while j < end:
                target[k] = source[j]
                j += 1
                k += 1
            start += 2 * width
        source, target = target, source
        width *= 2
    return source


def lower_bound(xs, value):
    lo = 0
    hi = len(xs)
    while lo < hi:
        mid = (lo + hi) // 2
        if xs[mid] < value:
            lo = mid + 1
        else:
            hi = mid
    return lo


def main():
    n = int(sys.argv[1])
    rng = Rng(41)
    records = []
    for i in range(n):
        r = rng.next()
        name = SYLLABLES[r % 16] + SYLLABLES[(r // 16) % 16] + SYLLABLES[(r // 256) % 16]
        records.append(Record(i, r % 20, 30000 + (r // 20) % 170000, name))

    salaries = [record.salary for record in records]
    sorted_salaries = list(salaries)
    sorted_salaries.sort()
    salary_check = 0
    for i, salary in enumerate(sorted_salaries):
        salary_check = (salary_check * 31 + salary * (i % 7 + 1)) % MOD

    by_key = list(records)
    by_key.sort(key=record_key)
    key_check = 0
    for record in by_key:
        key_check = (key_check * 31 + record.id) % MOD

    names = [record.name for record in records]
    names.sort()
    distinct = 1
    name_check = 0
    for i in range(len(names)):
        if i > 0 and names[i] != names[i - 1]:
            distinct += 1
        name_check = (name_check * 31 + len(names[i]) * (i % 5 + 1) + ord(names[i][0])) % MOD

    quick = list(salaries)
    quicksort(quick)
    quick_ok = quick == sorted_salaries

    order = merge_sort(list(range(n)), salaries)
    merge_check = 0
    for index in order:
        merge_check = (merge_check * 31 + index) % MOD

    search_sum = 0
    for _ in range(n):
        search_sum += lower_bound(sorted_salaries, 30000 + rng.next() % 170000)

    print(
        f"sort_records n={n} salary_check={salary_check} key_check={key_check} distinct={distinct} "
        f"name_check={name_check} quick_ok={str(quick_ok).lower()} merge_check={merge_check} search_sum={search_sum}"
    )


if __name__ == "__main__":
    main()
