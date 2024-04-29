use protocol::large_bedroom::LargeBedroom;

pub mod fast;
pub mod slow;

/// Higher prio will be send earlier
pub struct PriorityValue {
    priority: u8,
    pub value: protocol::large_bedroom::LargeBedroom,
}

impl PriorityValue {
    pub fn low_priority(&self) -> bool {
        self.priority > 0
    }
    fn p0(value: LargeBedroom) -> Self {
        Self { priority: 0, value }
    }
    fn p1(value: LargeBedroom) -> Self {
        Self { priority: 1, value }
    }

    fn p2(value: LargeBedroom) -> PriorityValue {
        Self { priority: 2, value }
    }
}

impl Eq for PriorityValue {}
impl PartialEq for PriorityValue {
    fn eq(&self, other: &Self) -> bool {
        self.priority.eq(&other.priority)
    }
}

impl PartialOrd for PriorityValue {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.priority.cmp(&other.priority))
    }
}

impl Ord for PriorityValue {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.priority.cmp(&other.priority)
    }
}
