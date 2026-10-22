//! When a card comes again: FSRS, the Free Spaced Repetition Scheduler
//! (version 5, Anki's scheduler since 23.10), with its default
//! parameters.
//!
//! A card's memory is its *stability* (the days until the chance to
//! remember it falls to 90 %) and its *difficulty* (1 to 10). Each answer
//! (*Again*, *Hard*, *Good*, *Easy*) changes both, by how likely the card
//! was remembered at that moment; the next review is when that chance
//! falls to the retention wanted (90 % unless set otherwise).
//!
//! In fixed point, 16 bits after the point ([`Fx`]): the X4 has no FPU,
//! and these few logarithms and powers need no floating point code. That
//! is within about 0.01 % of the formulas, far finer than a day.

/// How well a card was remembered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grade {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Grade {
    pub(crate) const ALL: [Grade; 4] = [Grade::Again, Grade::Hard, Grade::Good, Grade::Easy];
}

/// A number with 16 bits after the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Fx(i64);

const FRAC: u32 = 16;
const ONE: i64 = 1 << FRAC;

impl Fx {
    pub(crate) const ONE: Fx = Fx(ONE);

    /// `n` hundred-thousandths (the parameters' five decimals).
    const fn e5(n: i64) -> Fx {
        Fx((n * ONE + 50_000) / 100_000)
    }

    pub(crate) const fn int(n: i64) -> Fx {
        Fx(n << FRAC)
    }

    /// `n` hundredths, as cards keep their memory.
    pub(crate) const fn hundredths(n: i64) -> Fx {
        Fx((n * ONE + 50) / 100)
    }

    /// In hundredths, rounded.
    pub(crate) fn to_hundredths(self) -> i64 {
        (self.0 * 100 + ONE / 2) >> FRAC
    }

    /// Rounded to a whole number.
    pub(crate) fn round(self) -> i64 {
        (self.0 + ONE / 2) >> FRAC
    }

    /// `a` divided by `b`.
    pub(crate) fn ratio(a: i64, b: i64) -> Fx {
        Fx::int(a).div(Fx::int(b))
    }

    fn add(self, other: Fx) -> Fx {
        Fx(self.0.saturating_add(other.0))
    }

    fn sub(self, other: Fx) -> Fx {
        Fx(self.0.saturating_sub(other.0))
    }

    fn mul(self, other: Fx) -> Fx {
        let product = (i128::from(self.0) * i128::from(other.0)) >> FRAC;
        Fx(product.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
    }

    fn div(self, other: Fx) -> Fx {
        if other.0 == 0 {
            return Fx(i64::MAX);
        }
        let quotient = (i128::from(self.0) << FRAC) / i128::from(other.0);
        Fx(quotient.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64)
    }

    fn neg(self) -> Fx {
        Fx(-self.0)
    }

    fn clamp(self, low: Fx, high: Fx) -> Fx {
        Fx(self.0.clamp(low.0, high.0))
    }

    /// The base-2 logarithm, of a number above 0 (else that of the
    /// smallest).
    fn log2(self) -> Fx {
        let v = self.0.max(1) as u64;
        let msb = 63 - i64::from(v.leading_zeros());
        let mut result = (msb - i64::from(FRAC)) << FRAC;
        // The mantissa in [1, 2), 62 bits after the point; each squaring
        // gives a bit of the logarithm.
        let mut z = u128::from(v) << (62 - msb);
        for bit in (0..FRAC).rev() {
            z = (z * z) >> 62;
            if z >= 2 << 62 {
                z >>= 1;
                result += 1 << bit;
            }
        }
        Fx(result)
    }

    /// 2 to the power of this.
    fn exp2(self) -> Fx {
        // 2^(1/2), 2^(1/4), … 2^(1/65536), 30 bits after the point.
        const ROOTS: [u64; 16] = [
            1_518_500_250,
            1_276_901_417,
            1_170_923_762,
            1_121_280_436,
            1_097_253_708,
            1_085_434_106,
            1_079_572_136,
            1_076_653_033,
            1_075_196_443,
            1_074_468_888,
            1_074_105_294,
            1_073_923_544,
            1_073_832_680,
            1_073_787_251,
            1_073_764_537,
            1_073_753_181,
        ];
        let whole = self.0 >> FRAC;
        let fraction = self.0 & (ONE - 1);
        let mut r: u64 = 1 << 30;
        for (i, root) in ROOTS.iter().enumerate() {
            if fraction & (1 << (FRAC as usize - 1 - i)) != 0 {
                r = (r * root) >> 30;
            }
        }
        // From 30 bits after the point to 16, times 2^whole.
        let shift = whole - (30 - i64::from(FRAC));
        match shift {
            s if (0..32).contains(&s) => Fx((r << s) as i64),
            s if s >= 0 => Fx(i64::MAX),
            s if s > -64 => Fx((r >> -s) as i64),
            _ => Fx(0),
        }
    }

    /// e to the power of this.
    fn exp(self) -> Fx {
        // log2(e)
        self.mul(Fx(94_548)).exp2()
    }

    /// This (above 0) to the power of `p`.
    fn pow(self, p: Fx) -> Fx {
        p.mul(self.log2()).exp2()
    }

    /// The square root, of a number not below 0.
    fn sqrt(self) -> Fx {
        Fx(isqrt((self.0.max(0) as u64) << FRAC) as i64)
    }
}

/// The square root of `n`, rounded down.
fn isqrt(n: u64) -> u64 {
    if n < 2 {
        return n;
    }
    let mut x = 1u64 << (64 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// FSRS-5's default parameters, w0 to w18.
const W: [Fx; 19] = [
    Fx::e5(40_255),
    Fx::e5(118_385),
    Fx::e5(317_300),
    Fx::e5(1_569_105),
    Fx::e5(719_490),
    Fx::e5(53_450),
    Fx::e5(146_040),
    Fx::e5(460),
    Fx::e5(154_575),
    Fx::e5(11_920),
    Fx::e5(101_925),
    Fx::e5(193_950),
    Fx::e5(11_000),
    Fx::e5(29_605),
    Fx::e5(226_980),
    Fx::e5(23_150),
    Fx::e5(298_980),
    Fx::e5(51_655),
    Fx::e5(66_210),
];

/// The least stability kept: a hundredth of a day.
const LEAST_STABILITY: Fx = Fx::hundredths(1);
/// The longest interval, in days: a hundred years.
pub(crate) const MOST_DAYS: i64 = 36_500;
/// When a card answered *Again* comes again: in ten minutes.
pub(crate) const AGAIN_SECONDS: i64 = 10 * 60;
const DAY_SECONDS: i64 = 24 * 3600;

/// What is known of a card's memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Memory {
    /// In days.
    pub stability: Fx,
    /// 1 to 10.
    pub difficulty: Fx,
}

fn initial_difficulty(grade: Grade) -> Fx {
    let g = Fx::int(grade as i64 - 1);
    W[4].sub(W[5].mul(g).exp())
        .add(Fx::ONE)
        .clamp(Fx::ONE, Fx::int(10))
}

/// The chance to remember a card of `stability` after `days`.
pub(crate) fn retrievability(days: Fx, stability: Fx) -> Fx {
    // (1 + 19/81 t/S)^-0.5
    let factor = Fx::ratio(19, 81);
    Fx::ONE.div(Fx::ONE.add(factor.mul(days).div(stability)).sqrt())
}

fn next_difficulty(difficulty: Fx, grade: Grade) -> Fx {
    let delta = W[6].mul(Fx::int(3 - grade as i64));
    let damped = difficulty.add(delta.mul(Fx::int(10).sub(difficulty)).div(Fx::int(9)));
    W[7].mul(initial_difficulty(Grade::Easy))
        .add(Fx::ONE.sub(W[7]).mul(damped))
        .clamp(Fx::ONE, Fx::int(10))
}

fn stability_after_success(memory: Memory, r: Fx, grade: Grade) -> Fx {
    let Memory {
        stability: s,
        difficulty: d,
    } = memory;
    let mut growth = W[8]
        .exp()
        .mul(Fx::int(11).sub(d))
        .mul(s.pow(W[9].neg()))
        .mul(W[10].mul(Fx::ONE.sub(r)).exp().sub(Fx::ONE));
    match grade {
        Grade::Hard => growth = growth.mul(W[15]),
        Grade::Easy => growth = growth.mul(W[16]),
        _ => {}
    }
    s.mul(growth.add(Fx::ONE))
}

fn stability_after_failure(memory: Memory, r: Fx) -> Fx {
    let Memory {
        stability: s,
        difficulty: d,
    } = memory;
    let after = W[11]
        .mul(d.pow(W[12].neg()))
        .mul(s.add(Fx::ONE).pow(W[13]).sub(Fx::ONE))
        .mul(W[14].mul(Fx::ONE.sub(r)).exp());
    after.min(s)
}

/// Stability after a review on the same day (a card answered *Again*
/// and seen again): FSRS-5's short-term formula.
fn stability_same_day(stability: Fx, grade: Grade) -> Fx {
    let g = Fx::int(grade as i64 - 3).add(W[18]);
    stability.mul(W[17].mul(g).exp())
}

/// Days until the chance to remember falls to `retention`, at least one
/// and at most [`MOST_DAYS`].
pub(crate) fn interval_days(stability: Fx, retention: Fx) -> i64 {
    // S / (19/81) * (r^(1/-0.5) - 1)
    let inverse = Fx::ONE.div(retention.mul(retention)).sub(Fx::ONE);
    let days = stability.mul(Fx::ratio(81, 19)).mul(inverse);
    days.round().clamp(1, MOST_DAYS)
}

/// The answer `grade` to a card whose memory is `before` (`None`: a new
/// card) and that was last seen `elapsed` seconds ago: its memory after,
/// and in how many seconds it comes again, for `retention` wanted.
pub(crate) fn answer(
    before: Option<Memory>,
    elapsed: i64,
    grade: Grade,
    retention: Fx,
) -> (Memory, i64) {
    let memory = match before {
        None => Memory {
            stability: W[grade as usize - 1],
            difficulty: initial_difficulty(grade),
        },
        Some(memory) if elapsed < DAY_SECONDS => Memory {
            stability: stability_same_day(memory.stability, grade),
            difficulty: next_difficulty(memory.difficulty, grade),
        },
        Some(memory) => {
            let days = Fx::ratio(elapsed.max(0), DAY_SECONDS);
            let r = retrievability(days, memory.stability);
            Memory {
                stability: match grade {
                    Grade::Again => stability_after_failure(memory, r),
                    _ => stability_after_success(memory, r, grade),
                },
                difficulty: next_difficulty(memory.difficulty, grade),
            }
        }
    };
    let memory = Memory {
        stability: memory.stability.clamp(LEAST_STABILITY, Fx::int(MOST_DAYS)),
        ..memory
    };
    let due = match grade {
        Grade::Again => AGAIN_SECONDS,
        _ => interval_days(memory.stability, retention) * DAY_SECONDS,
    };
    (memory, due)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Within `percent` of `expected` (in hundred-thousandths, as the
    /// reference values are given).
    fn near(value: Fx, expected: f64, percent: f64) {
        let got = value.0 as f64 / ONE as f64;
        assert!(
            (got - expected).abs() <= expected.abs() * percent / 100.0 + 0.0001,
            "{got} is not near {expected}"
        );
    }

    #[test]
    fn logarithms_and_powers_are_exact_enough() {
        near(Fx::int(8).log2(), 3.0, 0.01);
        near(Fx::ratio(3, 2).log2(), 0.584_962_5, 0.01);
        near(Fx::ratio(1, 4).log2(), -2.0, 0.01);
        near(Fx::ratio(5, 2).exp2(), 5.656_854, 0.01);
        near(Fx::ratio(-3, 2).exp2(), 0.353_553, 0.05);
        near(W[8].exp(), 4.691_489, 0.01);
        near(W[2].pow(W[9].neg()), 0.871_414, 0.01);
        near(Fx::int(2).sqrt(), core::f64::consts::SQRT_2, 0.01);
        assert_eq!(isqrt(99), 9);
        assert_eq!(isqrt(u64::MAX), u64::from(u32::MAX));
    }

    #[test]
    fn new_cards_start_as_fsrs_says() {
        // Reference values from FSRS-5's formulas in floating point.
        let difficulties = [7.194_9, 6.488_305, 5.282_434, 3.224_502];
        for (grade, d) in Grade::ALL.into_iter().zip(difficulties) {
            let (memory, due) = answer(None, 0, grade, Fx::hundredths(90));
            near(memory.difficulty, d, 0.05);
            assert_eq!(memory.stability, W[grade as usize - 1]);
            let days = match grade {
                Grade::Again => None,
                _ => Some(due / DAY_SECONDS),
            };
            assert_eq!(
                days,
                [None, Some(1), Some(3), Some(16)][grade as usize - 1],
                "{grade:?}"
            );
        }
    }

    #[test]
    fn reviews_change_the_memory_as_fsrs_says() {
        let (good, _) = answer(None, 0, Grade::Good, Fx::hundredths(90));
        let three_days = 3 * DAY_SECONDS;
        near(retrievability(Fx::int(3), good.stability), 0.904_698, 0.01);
        let review = |grade| answer(Some(good), three_days, grade, Fx::hundredths(90));
        let (after, due) = review(Grade::Good);
        near(after.stability, 10.738_926, 0.1);
        near(after.difficulty, 5.272_968, 0.05);
        assert_eq!(due, 11 * DAY_SECONDS);
        near(review(Grade::Hard).0.stability, 4.924_512, 0.1);
        near(review(Grade::Easy).0.stability, 25.793_605, 0.1);
        let (lapsed, due) = review(Grade::Again);
        near(lapsed.stability, 1.055_561, 0.1);
        assert_eq!(due, AGAIN_SECONDS);
        // Ten days later, again good.
        let (later, _) = answer(
            Some(after),
            10 * DAY_SECONDS,
            Grade::Good,
            Fx::hundredths(90),
        );
        near(later.stability, 32.615_222, 0.2);
        // The same day, after Again: the short-term formula.
        let (again, _) = answer(None, 0, Grade::Again, Fx::hundredths(90));
        let (same_day, due) = answer(Some(again), 600, Grade::Good, Fx::hundredths(90));
        near(same_day.stability, 0.566_698, 0.1);
        assert_eq!(due, DAY_SECONDS, "at least a day");
    }

    #[test]
    fn intervals_follow_the_retention_wanted() {
        let s = Fx::int(10);
        assert_eq!(interval_days(s, Fx::hundredths(90)), 10);
        assert_eq!(interval_days(s, Fx::hundredths(85)), 16);
        assert_eq!(interval_days(s, Fx::hundredths(95)), 5);
        assert_eq!(
            interval_days(Fx::int(1_000_000), Fx::hundredths(70)),
            MOST_DAYS
        );
        assert_eq!(interval_days(Fx::hundredths(1), Fx::hundredths(90)), 1);
        // Whatever comes, the memory stays in its bounds.
        let mut memory = None;
        for (elapsed, grade) in [(0, Grade::Easy), (1 << 40, Grade::Easy), (1, Grade::Again)] {
            let (after, due) = answer(memory, elapsed, grade, Fx::hundredths(97));
            assert!(after.stability >= LEAST_STABILITY && after.stability <= Fx::int(MOST_DAYS));
            assert!(after.difficulty >= Fx::ONE && after.difficulty <= Fx::int(10));
            assert!(due > 0);
            memory = Some(after);
        }
    }
}
