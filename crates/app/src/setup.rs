use crate::state::Phase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Prepare,
    Allow,
    Open,
}

impl Step {
    pub const ALL: [Step; 3] = [Step::Prepare, Step::Allow, Step::Open];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mark {
    Todo,
    Working,
    WaitingOnPerson,
    Done,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    NotStarted,
    At { step: Step, mark: Mark },
}

impl Progress {
    pub fn of(phase: &Phase) -> Progress {
        let at = |step, mark| Progress::At { step, mark };
        match phase {
            Phase::Working(step) => at(*step, Mark::Working),
            Phase::NeedsResign(_) => at(Step::Prepare, Mark::WaitingOnPerson),
            Phase::Prepared(_) => at(Step::Prepare, Mark::Done),
            Phase::AwaitingApproval => at(Step::Allow, Mark::WaitingOnPerson),
            Phase::WaitingForClient(_) => at(Step::Open, Mark::WaitingOnPerson),
            Phase::SetupFailed { at: step, reason } => at(*step, Mark::Failed(reason.clone())),
            Phase::Connected(_) => at(Step::Open, Mark::Done),
            Phase::Failed(reason) => at(Step::Open, Mark::Failed(reason.clone())),
            Phase::Idle | Phase::Preparing(_) | Phase::Listening(_) | Phase::Ended { .. } => {
                Progress::NotStarted
            }
        }
    }

    pub fn step(&self) -> Option<Step> {
        match self {
            Progress::NotStarted => None,
            Progress::At { step, .. } => Some(*step),
        }
    }

    pub fn mark_of(&self, step: Step) -> Mark {
        match self {
            Progress::At { step: at, .. } if step.index() < at.index() => Mark::Done,
            Progress::At { step: at, mark } if step == *at => mark.clone(),
            _ => Mark::Todo,
        }
    }
}
