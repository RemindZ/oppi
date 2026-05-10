//! Layout constants and reservation-order helpers for mock parity.

use ratatui::layout::Rect;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ReservationBand {
    Header,
    Editor,
    Footer,
    Dock,
    Transcript,
    Overlay,
}

#[cfg(test)]
pub(super) const REFERENCE_RESERVATION_ORDER: [ReservationBand; 6] = [
    ReservationBand::Header,
    ReservationBand::Editor,
    ReservationBand::Footer,
    ReservationBand::Dock,
    ReservationBand::Transcript,
    ReservationBand::Overlay,
];

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CssCellTranslation {
    pub(super) css_rule: &'static str,
    pub(super) cell_rule: &'static str,
    pub(super) note: &'static str,
}

#[cfg(test)]
pub(super) const NON_REFERENCE_BORDER_POLICY: [&str; 4] = [
    "header uses text cells only, not a surrounding box",
    "transcript rows use gutters, not per-message boxes",
    "dock separator is a single rule row, not a boxed dock",
    "dock tray omits the bottom border so it welds to the editor",
];

#[cfg(test)]
pub(super) const TERMINAL_CSS_CELL_TRANSLATIONS: [CssCellTranslation; 8] = [
    CssCellTranslation {
        css_rule: ".term-body padding: 10px 14px 12px",
        cell_rule: "0 terminal cells in native mode",
        note: "browser mock padding is chrome-only; native snapshots own the full terminal grid",
    },
    CssCellTranslation {
        css_rule: ".term-header margin-bottom/padding-bottom/border-bottom",
        cell_rule: "1 normal row, 2 narrow rows, no extra dashed separator row",
        note: "reference terminal-output contracts encode header as plain grid rows",
    },
    CssCellTranslation {
        css_rule: ".tr-gutter width: 2ch",
        cell_rule: "2 cells",
        note: "TRANSCRIPT_GUTTER_WIDTH",
    },
    CssCellTranslation {
        css_rule: ".tr-label width: 8ch",
        cell_rule: "8 cells",
        note: "TRANSCRIPT_LABEL_WIDTH",
    },
    CssCellTranslation {
        css_rule: ".dock-sep border-top + right label",
        cell_rule: "1 rule row with right-aligned label",
        note: "render_dock_sep",
    },
    CssCellTranslation {
        css_rule: ".dock-tray border-bottom: none",
        cell_rule: "top/left/right borders only",
        note: "prevents doubled border above the editor",
    },
    CssCellTranslation {
        css_rule: ".editor padding + border",
        cell_rule: "3 rows in non-tiny fixture layouts",
        note: "top border, prompt row, bottom border",
    },
    CssCellTranslation {
        css_rule: ".term-footer rows/gap",
        cell_rule: "2 expanded rows or 1 collapsed row",
        note: "normal vs narrow/tiny terminal constraints",
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReferenceFrameLayout {
    pub(super) header: Rect,
    pub(super) transcript: Rect,
    pub(super) dock: Rect,
    pub(super) editor: Rect,
    pub(super) footer: Rect,
    pub(super) overlay: Rect,
}

pub(super) fn reference_frame_layout(
    area: Rect,
    header_height: u16,
    editor_height: u16,
    footer_height: u16,
    dock_height: u16,
) -> ReferenceFrameLayout {
    // Keep the reservation order aligned with `.reference/design-v2`: decide
    // header first, then fixed bottom bands (editor, footer, dock), then give
    // the remaining space to the bottom-anchored transcript. Spatial placement
    // still renders top-to-bottom as header/transcript/dock/editor/footer.
    let header_height = header_height.min(area.height);
    let header = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: header_height,
    };

    let body_y = area.y.saturating_add(header_height);
    let body_height = area.height.saturating_sub(header_height);
    let footer_height = footer_height.min(body_height);
    let editor_height = editor_height.min(body_height.saturating_sub(footer_height));
    let dock_height = dock_height.min(
        body_height
            .saturating_sub(footer_height)
            .saturating_sub(editor_height),
    );
    let transcript_height = body_height
        .saturating_sub(footer_height)
        .saturating_sub(editor_height)
        .saturating_sub(dock_height);

    let transcript = Rect {
        x: area.x,
        y: body_y,
        width: area.width,
        height: transcript_height,
    };
    let dock = Rect {
        x: area.x,
        y: body_y.saturating_add(transcript_height),
        width: area.width,
        height: dock_height,
    };
    let editor = Rect {
        x: area.x,
        y: dock.y.saturating_add(dock_height),
        width: area.width,
        height: editor_height,
    };
    let footer = Rect {
        x: area.x,
        y: editor.y.saturating_add(editor_height),
        width: area.width,
        height: footer_height,
    };

    ReferenceFrameLayout {
        header,
        transcript,
        dock,
        editor,
        footer,
        overlay: area,
    }
}
