/// The goal of this type is to search a large pool of values (eg Polkadot block numbers)
/// to locate a pair of blocks where a change occurs (eg the spec version changes).
#[derive(Debug)]
pub struct BinaryChopper<N, T> {
    min: (N, T),
    max: (N, T),
}

#[derive(Clone,PartialEq,Debug)]
pub enum Next<N, T> {
    NeedsState(N),
    Finished {
        min: (N, T),
        max: (N, T),
    }
}

impl <N, T> Next<N, T> {
    /// Unwrap [`Next`] and return the values from [`Next::Finished`].
    pub fn unwrap_finished(self) -> ((N, T), (N, T)) {
        match self {
            Next::Finished { min, max } => {
                (min, max)
            },
            _ => {
                panic!("Expected Next::Finished")
            }
        }
    }
}

impl <N: BinaryChopNumber, T: std::cmp::PartialEq + Clone> BinaryChopper<N, T> {
    /// Give an initial start and end value and state.
    pub fn new(min: (N, T), max: (N, T)) -> Self {
        Self { min, max }
    }

    /// Ask for the next value. This either returns [`Next::Finished`] to inidcate that
    /// it's found the pair of values with a state change, or it returns [`Next::NeedsState`]
    /// to indicate that you should turn the given number into some state, and then provide it
    /// via [`Self::set_state_for_next_value`]
    pub fn next_value(&self) -> Next<N, T> {
        // If we start with the same numbers, this will end. If the two numbers are
        // adjacent to eachother then we also end; no further chopping to do!
        if self.min.0 == self.max.0 || self.min.0.increment() == self.max.0 {
            Next::Finished { min: self.min.clone(), max: self.max.clone() }
        } else {
            Next::NeedsState(self.mid())
        }
    }

    /// Hand the state to the binary chopper that correpsonds to the value back from [`Self::next_value`].
    /// We then internally compare this with the other states and are either finished, or will propose the
    /// next number to test via the next call to [`Self::next_value()`].
    pub fn set_state_for_next_value(&mut self, state: T) {
        let mid = self.mid();
        if state == self.min.1 {
            self.min = (mid, state);
        } else {
            self.max = (mid, state);
        }
    }

    fn mid(&self) -> N {
        self.min.0.mid(&self.max.0)
    }
}

// Just a small trait so that we can be generic over a few number types in the above.
pub trait BinaryChopNumber: std::fmt::Debug + Copy + PartialEq {
    fn increment(&self) -> Self;
    fn mid(&self, other: &Self) -> Self;
}

macro_rules! impl_binary_chop_number {
    ($ty:ty) => {
        impl BinaryChopNumber for $ty {
            fn increment(&self) -> Self {
                self + 1
            }
            fn mid(&self, other: &Self) -> Self {
                (self + other) / 2
            }
        }
    }
}

impl_binary_chop_number!(usize);
impl_binary_chop_number!(u64);
impl_binary_chop_number!(u32);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_going_higher() {
        let versions = vec![0,0,0,1,1,1,1,2,3,4,4,5];
        let mut start = 0usize;
        let end = versions.len() - 1;
        let mut changes = vec![];

        while start != end {
            let mut chopper = BinaryChopper::new(
                (start, versions[start]), 
                (end, versions[end]),
            );
    
            while let Next::NeedsState(n) = chopper.next_value() {
                chopper.set_state_for_next_value(versions[n as usize]);
            }

            let finished = chopper.next_value().unwrap_finished();
            let ((_change_start, _start_state), (change_end, _end_state)) = finished;

            start = change_end;
            changes.push(finished);
        }

        // We should find all of the indexes at which the values change:
        assert_eq!(
            changes,
            vec![
                ((2, 0), (3, 1)),
                ((6, 1), (7, 2)),
                ((7, 2), (8, 3)),
                ((8, 3), (9, 4)),
                ((10, 4), (11, 5)),
            ]
        );
    }
}