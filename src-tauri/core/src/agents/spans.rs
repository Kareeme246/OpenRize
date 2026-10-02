//! Half-open time spans `[start, end)` in epoch milliseconds, and the set
//! algebra the billing rules are built from. Every function takes and returns
//! *normalized* sets: sorted, non-empty, and neither overlapping nor touching.

/// A half-open interval of epoch milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Span {
    pub start: u64,
    pub end: u64,
}

impl Span {
    pub fn new(start: u64, end: u64) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }
}

/// Sorts, drops empty spans and merges overlapping or touching ones.
pub fn normalize(spans: &[Span]) -> Vec<Span> {
    let mut sorted: Vec<Span> = spans.iter().copied().filter(|s| !s.is_empty()).collect();
    sorted.sort();
    let mut merged: Vec<Span> = Vec::with_capacity(sorted.len());
    for span in sorted {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }
    merged
}

pub fn union(a: &[Span], b: &[Span]) -> Vec<Span> {
    let mut both = a.to_vec();
    both.extend_from_slice(b);
    normalize(&both)
}

/// Where `a` and `b` overlap. Both inputs must be normalized.
pub fn intersect(a: &[Span], b: &[Span]) -> Vec<Span> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        let start = a[i].start.max(b[j].start);
        let end = a[i].end.min(b[j].end);
        if start < end {
            out.push(Span::new(start, end));
        }
        if a[i].end < b[j].end {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// What is left of `a` once `b` is cut out. Both inputs must be normalized.
pub fn subtract(a: &[Span], b: &[Span]) -> Vec<Span> {
    let mut out = Vec::new();
    let mut j = 0;
    for span in a {
        let mut start = span.start;
        while j < b.len() && b[j].end <= start {
            j += 1;
        }
        let mut k = j;
        while k < b.len() && b[k].start < span.end {
            if b[k].start > start {
                out.push(Span::new(start, b[k].start));
            }
            start = start.max(b[k].end);
            k += 1;
        }
        if start < span.end {
            out.push(Span::new(start, span.end));
        }
    }
    out
}

/// The part of each span inside `[start, end)`.
pub fn clip(spans: &[Span], start: u64, end: u64) -> Vec<Span> {
    intersect(spans, &[Span::new(start, end)])
}

pub fn total(spans: &[Span]) -> u64 {
    spans.iter().map(Span::len).sum()
}

/// Whether `at` falls inside any span of a normalized set.
pub fn covers(spans: &[Span], at: u64) -> bool {
    spans
        .binary_search_by(|span| {
            if span.end <= at {
                std::cmp::Ordering::Less
            } else if span.start > at {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(start: u64, end: u64) -> Span {
        Span::new(start, end)
    }

    #[test]
    fn normalize_merges_touching_and_drops_empty() {
        let merged = normalize(&[s(10, 20), s(0, 5), s(5, 8), s(30, 30), s(15, 25)]);
        assert_eq!(merged, vec![s(0, 8), s(10, 25)]);
    }

    #[test]
    fn intersect_and_subtract_partition_a_set() {
        let a = normalize(&[s(0, 100)]);
        let b = normalize(&[s(10, 20), s(50, 60), s(90, 120)]);
        assert_eq!(intersect(&a, &b), vec![s(10, 20), s(50, 60), s(90, 100)]);
        assert_eq!(subtract(&a, &b), vec![s(0, 10), s(20, 50), s(60, 90)]);
        assert_eq!(
            total(&intersect(&a, &b)) + total(&subtract(&a, &b)),
            total(&a)
        );
    }

    #[test]
    fn subtracting_everything_leaves_nothing() {
        assert!(subtract(&[s(5, 10)], &[s(0, 20)]).is_empty());
    }

    #[test]
    fn covers_finds_points_in_a_set() {
        let set = normalize(&[s(0, 10), s(20, 30)]);
        assert!(covers(&set, 0));
        assert!(covers(&set, 29));
        assert!(!covers(&set, 10));
        assert!(!covers(&set, 15));
    }
}
