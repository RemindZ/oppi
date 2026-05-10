//! Footer widget: status, usage bars, model, permission, context, todos, hotkeys.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) enum FooterCell {
    Status,
    SessionUsage,
    WeeklyUsage,
    Model,
    Permission,
    Context,
    TodosQueued,
    Hotkeys,
}

pub(in super::super) const FOOTER_CELLS: [FooterCell; 8] = [
    FooterCell::Status,
    FooterCell::SessionUsage,
    FooterCell::WeeklyUsage,
    FooterCell::Model,
    FooterCell::Permission,
    FooterCell::Context,
    FooterCell::TodosQueued,
    FooterCell::Hotkeys,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in super::super) struct FooterVisualContract {
    pub(in super::super) expanded_height: u16,
    pub(in super::super) collapsed_height: u16,
    pub(in super::super) ready_dot: &'static str,
    pub(in super::super) running_dot: &'static str,
    pub(in super::super) session_bar: &'static str,
    pub(in super::super) week_bar: &'static str,
    pub(in super::super) context_bar: &'static str,
    pub(in super::super) todos_prefix: &'static str,
}

pub(in super::super) const FOOTER_VISUAL_CONTRACT: FooterVisualContract = FooterVisualContract {
    expanded_height: 2,
    collapsed_height: 1,
    ready_dot: "•",
    running_dot: "●",
    session_bar: "▮▮▮▯▯",
    week_bar: "▮▯▯▯▯",
    context_bar: "▮▮▯▯▯",
    todos_prefix: "todos ",
};
