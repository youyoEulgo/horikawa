use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    prelude::*,
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

use crate::view::{ViewMode, VisualizerStyle};

pub fn handle(app: &mut crate::App, code: KeyCode, modifiers: KeyModifiers) {
    if app.handle_view_jump(code) {
        return;
    }
    match code {
        KeyCode::Char('q') => {
            app.should_quit = true;
        }
        KeyCode::Esc => {
            app.view_mode = ViewMode::Playlist;
        }
        KeyCode::Char('s') => {
            app.visualizer_style = app.visualizer_style.next();
            app.set_status(format!("Visualizer: {}", app.visualizer_style.name()));
        }
        KeyCode::Char('f') => {
            app.spectrum_mode = !app.spectrum_mode;
            app.set_status(if app.spectrum_mode {
                "Spectrum mode: FFT"
            } else {
                "Spectrum mode: Volume"
            });
        }
        _ if app.handle_playback_key(code, modifiers) => {}
        _ => {}
    }
}



pub fn draw(frame: &mut Frame, app: &crate::App, area: Rect) {
    let vis_data = if app.spectrum_mode {
        app.player.vis_data()
    } else {
        app.player.vis_rms()
    };

    // Use the full height of the content area for visualization
    let inner_height = area.height.saturating_sub(2) as usize; // Account for borders
    let inner_width = area.width.saturating_sub(2) as usize;

    let mut lines = Vec::with_capacity(inner_height);

    if let Some(data) = vis_data {
        match app.visualizer_style {
            VisualizerStyle::Bars => {
                draw_vis_bars(&mut lines, &data, inner_height, inner_width);
            }
            VisualizerStyle::Spectrum => {
                draw_vis_spectrum(&mut lines, &data, inner_height, inner_width);
            }
            VisualizerStyle::Waveform => {
                draw_vis_waveform(&mut lines, &data, inner_height, inner_width);
            }
            VisualizerStyle::LevelMeter => {
                draw_vis_level_meter(&mut lines, &data, inner_height, inner_width);
            }
        }
    } else {
        // No audio data - show a message
        let msg = "No audio playing";
        let padding = (inner_height / 2).saturating_sub(1);
        for _ in 0..padding {
            lines.push(Line::from(""));
        }
        lines.push(Line::from(Span::styled(
            msg,
            Style::default().fg(Color::DarkGray).italic(),
        )));
    }

    let title = format!(
        " Visualizer: {} (s:style, f:FFT/Vol, v/Esc:close) ",
        app.visualizer_style.name()
    );
    let visualizer = Paragraph::new(lines)
        .block(Block::default().title(title).borders(Borders::ALL))
        .alignment(Alignment::Center);

    frame.render_widget(visualizer, area);
}



pub fn draw_vis_bars(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    let vis_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    // Pick a fixed bar width based on terminal width.
    let bw = if width >= 200 {
        3u16
    } else if width >= 100 {
        2
    } else {
        1
    };
    let gap = 1u16;
    let total_per_bar = bw + gap;
    let num_bars = (width as u16 / total_per_bar)
        .min(data.len().max(1) as u16)
        .max(1) as usize;
    let total_width = (num_bars as u16 * total_per_bar).saturating_sub(gap) as usize;
    let pad_left = (width.saturating_sub(total_width)) / 2;

    for row in (0..height).rev() {
        let threshold = (row as f32 + 0.5) / height as f32;
        let mut line_content = String::with_capacity(width);
        for _ in 0..pad_left {
            line_content.push(' ');
        }

        for bar_idx in 0..num_bars {
            // Map bar to spectrum range with RMS
            let start = (bar_idx * data.len()) / num_bars;
            let end = ((bar_idx + 1) * data.len()) / num_bars;
            let amp: f32 =
                data[start..end].iter().map(|s| s * s).sum::<f32>() / (end - start) as f32;
            let scaled_amp = amp.sqrt().powf(0.35).min(1.0);

            if scaled_amp >= threshold {
                let level = (((scaled_amp - threshold) * height as f32 * 8.0) as usize).min(7);
                for _ in 0..bw {
                    line_content.push(vis_chars[level]);
                }
            } else {
                for _ in 0..bw {
                    line_content.push(' ');
                }
            }
            if bar_idx + 1 < num_bars {
                for _ in 0..gap {
                    line_content.push(' ');
                }
            }
        }

        lines.push(Line::from(Span::styled(
            line_content,
            Style::default().fg(Color::Cyan),
        )));
    }
}



pub fn draw_vis_spectrum(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    let vis_chars = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let bw = if width >= 200 {
        3u16
    } else if width >= 100 {
        2
    } else {
        1
    };
    let gap = 1u16;
    let total_per_bar = bw + gap;
    let num_bars = (width as u16 / total_per_bar)
        .min(data.len().max(1) as u16)
        .max(1) as usize;
    let total_width = (num_bars as u16 * total_per_bar).saturating_sub(gap) as usize;
    let pad_left = (width.saturating_sub(total_width)) / 2;
    let half_height = height / 2;

    // Draw mirrored spectrum.
    //
    // Top half: lower block chars fill from cell bottom (which sits on the
    // center line), so bars naturally grow UP from center.
    //
    // Bottom half: line-level mirror of the top half. Lower block chars fill
    // from the cell bottom, which in the bottom half is away from center.
    // The silhouette (envelope of bar heights) mirrors correctly; partial-fill
    // characters within individual cells may appear to grow from the opposite
    // direction, but the overall shape is symmetric.
    let mut top_lines: Vec<String> = Vec::with_capacity(half_height);

    for row in (0..half_height).rev() {
        let threshold = (row as f32 + 0.5) / half_height as f32;
        let mut line_content = String::with_capacity(width);
        for _ in 0..pad_left {
            line_content.push(' ');
        }

        for bar_idx in 0..num_bars {
            let start = (bar_idx * data.len()) / num_bars;
            let end = ((bar_idx + 1) * data.len()) / num_bars;
            let amp: f32 =
                data[start..end].iter().map(|s| s * s).sum::<f32>() / (end - start) as f32;
            let scaled_amp = amp.sqrt().powf(0.35).min(1.0);

            if scaled_amp >= threshold {
                let level = (((scaled_amp - threshold) * half_height as f32 * 8.0) as usize).min(7);
                for _ in 0..bw {
                    line_content.push(vis_chars[level]);
                }
            } else {
                for _ in 0..bw {
                    line_content.push(' ');
                }
            }
            if bar_idx + 1 < num_bars {
                for _ in 0..gap {
                    line_content.push(' ');
                }
            }
        }
        top_lines.push(line_content);
    }

    for line in top_lines.iter() {
        lines.push(Line::from(Span::styled(
            line.clone(),
            Style::default().fg(Color::Magenta),
        )));
    }
    for line in top_lines.iter().rev() {
        lines.push(Line::from(Span::styled(
            line.clone(),
            Style::default().fg(Color::Cyan),
        )));
    }
}



pub fn draw_vis_waveform(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    let center_row = height / 2;

    // Build the waveform grid
    let mut grid: Vec<Vec<char>> = vec![vec![' '; width]; height];

    for x in 0..width {
        let data_idx = (x * data.len()) / width;
        let amp = data[data_idx.min(data.len() - 1)];

        // Convert amplitude to y offset from center
        let y_offset = (amp.powf(0.35) * center_row as f32) as isize;
        let y = (center_row as isize - y_offset).clamp(0, (height - 1) as isize) as usize;

        grid[y][x] = '●';

        // Draw vertical line from center to point
        let start_y = center_row.min(y);
        let end_y = center_row.max(y);
        for row in start_y..=end_y {
            if grid[row][x] == ' ' {
                grid[row][x] = '│';
            }
        }
    }

    // Draw center line
    for x in 0..width {
        if grid[center_row][x] == ' ' {
            grid[center_row][x] = '─';
        }
    }

    // Convert grid to lines
    for row in &grid {
        let line_str: String = row.iter().collect();
        lines.push(Line::from(Span::styled(
            line_str,
            Style::default().fg(Color::Green),
        )));
    }
}



pub fn draw_vis_level_meter(lines: &mut Vec<Line<'static>>, data: &[f32], height: usize, width: usize) {
    // Average amplitude for left and right channels (simple stereo simulation)
    let mid = data.len() / 2;
    let left_amp: f32 = data[..mid].iter().sum::<f32>() / mid.max(1) as f32;
    let right_amp: f32 = data[mid..].iter().sum::<f32>() / (data.len() - mid).max(1) as f32;
    let total_amp: f32 = data.iter().sum::<f32>() / data.len().max(1) as f32;

    let meter_width = width.saturating_sub(10);
    let left_filled = (left_amp.powf(0.35) * meter_width as f32) as usize;
    let right_filled = (right_amp.powf(0.35) * meter_width as f32) as usize;
    let total_filled = (total_amp.powf(0.35) * meter_width as f32) as usize;

    // Create meter characters
    let create_meter = |filled: usize, total: usize| -> String {
        let mut result = String::new();
        for i in 0..total {
            if i < filled {
                result.push('█');
            } else {
                result.push('░');
            }
        }
        result
    };

    // Pad vertically to center
    let content_height = 7;
    let padding = (height.saturating_sub(content_height)) / 2;

    for _ in 0..padding {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  L  [", Style::default().fg(Color::Gray)),
        Span::styled(
            create_meter(left_filled, meter_width),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled("]", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  R  [", Style::default().fg(Color::Gray)),
        Span::styled(
            create_meter(right_filled, meter_width),
            Style::default().fg(Color::Magenta),
        ),
        Span::styled("]", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" Mix [", Style::default().fg(Color::Gray)),
        Span::styled(
            create_meter(total_filled, meter_width),
            Style::default().fg(Color::Green),
        ),
        Span::styled("]", Style::default().fg(Color::Gray)),
    ]));
    lines.push(Line::from(""));
}

