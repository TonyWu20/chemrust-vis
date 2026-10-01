//! Status bar widget showing file name, atom count, and camera state.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

/// Status bar showing file name, atom count, and camera parameters.
pub struct StatusBar {
    pub file_name: String,
    pub atom_count: usize,
    pub theta_deg: f64,
    pub phi_deg: f64,
    pub radius: f64,
}

impl Widget for StatusBar {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let text = Line::from(vec![
            Span::styled(
                format!(" {} ", self.file_name),
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ),
            Span::raw(format!(
                " | {} atoms | θ={:.0}° φ={:.0}° r={:.1}",
                self.atom_count, self.theta_deg, self.phi_deg, self.radius
            )),
        ]);
        Paragraph::new(text).render(area, buf);
    }
}
