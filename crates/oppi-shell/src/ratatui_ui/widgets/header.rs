//! Header widget: spinner, OPPi identity, provider/model, permission/status, thread/goal cell.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum HeaderCell {
    Spinner,
    Identity,
    ProviderModel,
    Permission,
    Status,
    ThreadOrGoal,
}

pub(in super::super) const HEADER_CELLS: [HeaderCell; 6] = [
    HeaderCell::Spinner,
    HeaderCell::Identity,
    HeaderCell::ProviderModel,
    HeaderCell::Permission,
    HeaderCell::Status,
    HeaderCell::ThreadOrGoal,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct HeaderVisualContract {
    pub(in super::super) separator: &'static str,
    pub(in super::super) ready_spinner: &'static str,
    pub(in super::super) running_spinner: &'static str,
    pub(in super::super) waiting_spinner: &'static str,
    pub(in super::super) goal_prefix: &'static str,
    pub(in super::super) normal_height: u16,
    pub(in super::super) narrow_height: u16,
}

pub(in super::super) const HEADER_VISUAL_CONTRACT: HeaderVisualContract = HeaderVisualContract {
    separator: " · ",
    ready_spinner: "•",
    running_spinner: "◐",
    waiting_spinner: "⏸",
    goal_prefix: "◎ ",
    normal_height: 1,
    narrow_height: 2,
};
