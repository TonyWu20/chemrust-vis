//! Status bar widget showing file name, atom count, camera state,
//! the active scene renderer, and the kitty frame pixel size.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

/// Status bar showing file name, atom count, camera parameters,
/// renderer, and snipe mode.
pub struct StatusBar {
    pub file_name: String,
    pub atom_count: usize,
    pub theta_deg: f64,
    pub phi_deg: f64,
    pub radius: f64,
    /// Active scene renderer: "kitty" or "half-block".
    pub renderer: &'static str,
    /// Last transmitted kitty frame size in pixels. `None` for the
    /// half-block renderer, or before the first kitty frame.
    pub kitty_px: Option<(u32, u32)>,
    /// Snipe mode active: orbit and move-zoom run at 1/10 sensitivity.
    pub sniped: bool,
}

impl Widget for StatusBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut spans = vec![
            Span::styled(
                format!(" {} ", self.file_name),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Span::raw(format!(
                " | {} atoms | θ={:.0}° φ={:.0}° r={:.1} | {}",
                self.atom_count,
                self.theta_deg,
                self.phi_deg,
                self.radius,
                self.renderer
            )),
        ];
        if let Some((w, h)) = self.kitty_px {
            spans.push(Span::raw(format!(" {w}\u{00d7}{h}px")));
        }
        if self.sniped {
            spans.push(Span::raw(" | "));
            spans.push(Span::styled("snipe", Style::default().fg(Color::Yellow)));
        }
        Paragraph::new(Line::from(spans)).render(area, buf);
    }
}
