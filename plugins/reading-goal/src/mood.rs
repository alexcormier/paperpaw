//! The cat's mood, by how much of the goal was read today.

/// How the cat feels about today's reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// Nothing read yet (less than a minute).
    Sad,
    /// Some, less than [`CONTENT`](crate::CONTENT) of the goal.
    Meh,
    /// From that share on, below [`HAPPY`](crate::HAPPY).
    Content,
    /// The goal reached.
    Happy,
}

impl Mood {
    pub const ALL: [Mood; 4] = [Mood::Sad, Mood::Meh, Mood::Content, Mood::Happy];

    /// After `seconds` read of a goal of `goal` minutes, content from
    /// `content` percent of it, happy from `happy` percent.
    pub fn of(seconds: u32, goal: u32, content: u32, happy: u32) -> Mood {
        if seconds < 60 {
            return Mood::Sad;
        }
        let goal = u64::from(goal.max(1)) * 60;
        let percent = u64::from(seconds) * 100 / goal;
        if percent >= u64::from(happy) {
            Mood::Happy
        } else if percent >= u64::from(content) {
            Mood::Content
        } else {
            Mood::Meh
        }
    }

    /// Its name: of its picture file and its translations.
    pub fn name(self) -> &'static str {
        match self {
            Mood::Sad => "sad",
            Mood::Meh => "meh",
            Mood::Content => "content",
            Mood::Happy => "happy",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moods_follow_the_share_of_the_goal() {
        let of = |minutes: u32| Mood::of(minutes * 60, 20, 50, 100);
        assert_eq!(Mood::of(59, 20, 50, 100), Mood::Sad);
        assert_eq!(of(0), Mood::Sad);
        assert_eq!(of(1), Mood::Meh);
        assert_eq!(of(9), Mood::Meh);
        assert_eq!(of(10), Mood::Content);
        assert_eq!(of(19), Mood::Content);
        assert_eq!(of(20), Mood::Happy);
        assert_eq!(of(500), Mood::Happy);
        // Thresholds as set.
        assert_eq!(Mood::of(15 * 60, 20, 80, 150), Mood::Meh);
        assert_eq!(Mood::of(29 * 60, 20, 80, 150), Mood::Content);
        assert_eq!(Mood::of(30 * 60, 20, 80, 150), Mood::Happy);
        assert_eq!(Mood::of(u32::MAX, 0, 50, 100), Mood::Happy);
    }
}
