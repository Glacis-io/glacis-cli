use crossterm::{
    event::{self, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph},
};
use std::{io, time::Duration};
use tokio::time::sleep;
use rand::Rng;

// --- BRAND COLORS: INSTITUTIONAL, NOT HACKER ---
const GLACIS_CYAN: Color = Color::Rgb(6, 182, 212);      // Cyan-500 (Primary Brand)
const GLACIS_PURPLE: Color = Color::Rgb(139, 92, 246);   // Purple-500 (Gradient End)
const GLACIS_SLATE: Color = Color::Rgb(148, 163, 184);   // Slate-400 (Borders)
const GLACIS_SLATE_DARK: Color = Color::Rgb(30, 41, 59); // Slate-800 (Grid)
const GLACIS_ALERT: Color = Color::Rgb(245, 158, 11);    // Amber-500 (Status)
const GLACIS_SUCCESS: Color = Color::Rgb(45, 212, 191);  // Teal-400 (Clinical Success)

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup Terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // 2. Run the "Cold Boot" Sequence
    run_cold_boot(&mut terminal).await?;

    // 3. Run the Cinematic Dashboard (The Loop with Lidar Background)
    run_cinematic_interface(&mut terminal).await?;

    // 4. Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

async fn run_cold_boot<B: Backend>(terminal: &mut Terminal<B>) -> io::Result<()> {
    // LIDAR-style boot: Radar sweep reveals a depth-mapped "G" logo
    let total_frames = 180; // ~3 seconds at 60fps
    let mut rng = rand::thread_rng();

    // Define the "G" shape as a depth map (30x30 grid)
    // Each value represents depth: 0=background, 1-9=depth layers
    #[rustfmt::skip]
    let g_depth_map: [[u8; 30]; 30] = [
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,2,3,4,5,6,6,6,6,5,4,3,2,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,3,5,7,8,9,9,9,9,9,9,9,8,7,5,3,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,4,7,8,9,9,9,9,9,9,9,9,9,9,9,8,6,3,0,0,0,0,0,0,0,0],
        [0,0,0,0,3,6,9,9,9,9,8,7,5,4,3,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,5,8,9,9,9,7,5,3,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,2,7,9,9,9,8,5,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,3,8,9,9,9,6,3,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,2,5,9,9,9,8,5,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,3,6,9,9,9,7,4,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,4,7,9,9,9,6,3,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,4,8,9,9,9,5,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,5,8,9,9,9,5,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,5,8,9,9,9,5,2,0,0,0,0,0,3,4,5,6,7,8,9,9,8,6,4,0,0,0,0,0],
        [0,0,5,8,9,9,9,5,2,0,0,0,0,0,5,7,8,9,9,9,9,9,9,9,7,4,0,0,0,0],
        [0,0,4,8,9,9,9,5,2,0,0,0,0,0,6,8,9,9,9,9,9,9,9,9,9,6,3,0,0,0],
        [0,0,4,7,9,9,9,6,3,0,0,0,0,0,0,0,0,0,0,0,4,7,9,9,9,8,5,2,0,0],
        [0,0,3,7,9,9,9,7,4,0,0,0,0,0,0,0,0,0,0,0,0,3,6,9,9,9,7,4,0,0],
        [0,0,2,5,9,9,9,8,5,2,0,0,0,0,0,0,0,0,0,0,0,0,4,8,9,9,8,5,2,0],
        [0,0,0,4,8,9,9,9,7,4,0,0,0,0,0,0,0,0,0,0,0,0,3,7,9,9,9,6,3,0],
        [0,0,0,2,7,9,9,9,8,6,3,0,0,0,0,0,0,0,0,0,0,2,5,8,9,9,8,5,2,0],
        [0,0,0,0,5,8,9,9,9,8,5,3,0,0,0,0,0,0,0,0,3,5,8,9,9,9,7,4,0,0],
        [0,0,0,0,3,7,9,9,9,9,8,6,4,2,0,0,0,2,4,6,8,9,9,9,9,7,4,0,0,0],
        [0,0,0,0,0,4,8,9,9,9,9,9,9,8,7,6,7,8,9,9,9,9,9,9,8,5,2,0,0,0],
        [0,0,0,0,0,0,3,7,9,9,9,9,9,9,9,9,9,9,9,9,9,9,9,8,5,2,0,0,0,0],
        [0,0,0,0,0,0,0,2,5,8,9,9,9,9,9,9,9,9,9,9,9,9,7,4,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,2,4,6,7,8,8,8,8,7,6,5,3,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
        [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    ];

    for frame in 0..total_frames {
        terminal.draw(|f| {
            let size = f.size();
            let buffer = f.buffer_mut();

            // Calculate radar sweep angle (0-360 degrees, completes 2 full rotations)
            let sweep_angle = (frame as f64 / total_frames as f64) * 720.0;
            let sweep_rad = sweep_angle.to_radians();

            // Center of screen
            let center_x = size.width / 2;
            let center_y = size.height / 2;

            // Draw LIDAR point cloud
            for grid_y in 0..30 {
                for grid_x in 0..30 {
                    let depth = g_depth_map[grid_y][grid_x];
                    if depth == 0 { continue; }

                    // Map grid to screen coordinates (offset from center)
                    let screen_x = center_x as i32 + (grid_x as i32 - 15) * 2;
                    let screen_y = center_y as i32 + (grid_y as i32 - 15);

                    if screen_x < 0 || screen_y < 0 || screen_x >= size.width as i32 || screen_y >= size.height as i32 {
                        continue;
                    }

                    // Calculate angle from center to this point
                    let dx = (grid_x as f64 - 15.0) * 2.0;
                    let dy = grid_y as f64 - 15.0;
                    let point_angle = dy.atan2(dx).to_degrees() + 180.0;

                    // Check if radar sweep has revealed this point
                    let angle_diff = (point_angle - sweep_angle).abs() % 360.0;
                    let revealed = angle_diff < sweep_angle || sweep_angle > 360.0;

                    if revealed {
                        let cell = buffer.get_mut(screen_x as u16, screen_y as u16);

                        // Depth-based character selection (photographic gradient)
                        let depth_char = match depth {
                            0 => ' ',
                            1 => '·',
                            2 => '.',
                            3 => ':',
                            4 => ';',
                            5 => '!',
                            6 => '*',
                            7 => '◆',
                            8 => '▓',
                            _ => '█',
                        };

                        // Heat map color: dark (far) to bright cyan/white (near)
                        let heat_color = match depth {
                            0 => Color::Black,
                            1 => Color::Rgb(10, 25, 35),
                            2 => Color::Rgb(15, 40, 55),
                            3 => Color::Rgb(20, 60, 80),
                            4 => Color::Rgb(25, 90, 110),
                            5 => Color::Rgb(30, 120, 140),
                            6 => Color::Rgb(40, 150, 170),
                            7 => Color::Rgb(60, 180, 200),
                            8 => Color::Rgb(100, 210, 230),
                            _ => Color::Rgb(200, 240, 255),
                        };

                        // Add slight noise/variation for photographic effect
                        let noise = if rng.gen_bool(0.15) {
                            if rng.gen_bool(0.5) { 1 } else { -1 }
                        } else { 0 };

                        let final_color = if let Color::Rgb(r, g, b) = heat_color {
                            Color::Rgb(
                                (r as i16 + noise * 5).max(0).min(255) as u8,
                                (g as i16 + noise * 5).max(0).min(255) as u8,
                                (b as i16 + noise * 5).max(0).min(255) as u8,
                            )
                        } else {
                            heat_color
                        };

                        cell.set_char(depth_char);
                        cell.set_fg(final_color);

                        // Brighten points near the sweep line (active scan effect)
                        if angle_diff < 30.0 {
                            cell.set_fg(Color::Rgb(220, 250, 255));
                        }
                    }
                }
            }

            // Draw radar sweep line
            for r in 0..50 {
                let px = center_x as f64 + (r as f64 * sweep_rad.cos());
                let py = center_y as f64 + (r as f64 * sweep_rad.sin());

                if px >= 0.0 && py >= 0.0 && (px as u16) < size.width && (py as u16) < size.height {
                    let cell = buffer.get_mut(px as u16, py as u16);
                    let intensity = 255 - (r as u8 * 3);
                    cell.set_char('│');
                    cell.set_fg(Color::Rgb(intensity, 255, 255));
                }
            }

            // Status text at bottom
            let progress = ((frame as f64 / total_frames as f64) * 100.0) as u16;
            let status_text = format!("LIDAR IMAGING: {}% | GLACIS CHAIN OF CUSTODY", progress);
            let status_y = size.height - 3;

            for (i, ch) in status_text.chars().enumerate() {
                let x = (size.width / 2).saturating_sub(status_text.len() as u16 / 2) + i as u16;
                if x < size.width {
                    let cell = buffer.get_mut(x, status_y);
                    cell.set_char(ch);
                    cell.set_fg(GLACIS_CYAN);
                }
            }
        })?;

        sleep(Duration::from_millis(16)).await; // 60fps
    }

    sleep(Duration::from_millis(500)).await; // Pause before transition
    Ok(())
}

struct AppState {
    logs: Vec<(String, u32)>, // (log_text, age_in_frames)
    receipt_count: u64,
    frame_count: u64,
    scan_line_y: u16,
    grid_twinkle_map: Vec<(u16, u16, u8)>, // (x, y, intensity) for persistent twinkling
    counter_blur_offset: i32, // For the blurred counter effect
}

async fn run_cinematic_interface<B: Backend>(terminal: &mut Terminal<B>) -> io::Result<()> {
    let mut state = AppState {
        logs: vec![
            ("[14:32:01] 0x8f3a...42c1 | POL-489 | ACTIVE".to_string(), 50),
            ("[14:32:03] 0x7b21...9af4 | POL-490 | ACTIVE".to_string(), 40),
            ("[14:32:07] 0x3c85...1de2 | POL-491 | ACTIVE".to_string(), 30),
        ],
        receipt_count: 1402,
        frame_count: 0,
        scan_line_y: 0,
        grid_twinkle_map: Vec::new(),
        counter_blur_offset: 0,
    };

    let mut rng = rand::thread_rng();
    let mut last_log_frame = 0;

    loop {
        terminal.draw(|f| ui(f, &mut state))?;

        // 1. Event Handling (Quit)
        if event::poll(Duration::from_millis(16))? {
            if let event::Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q') {
                    return Ok(());
                }
            }
        }

        // 2. AGGRESSIVE ANIMATION LOGIC
        state.frame_count += 1;

        // Age all logs (faster decay for more dramatic fade)
        for (_log, age) in state.logs.iter_mut() {
            *age += 2; // Double speed aging for faster fade
        }

        // Scanline movement (faster sweep - moves 3 units per frame)
        state.scan_line_y = (state.scan_line_y + 3) % 100;

        // HYPER-FAST receipt counter - ALWAYS ticking (blurred effect)
        state.receipt_count += rng.gen_range(1..5);
        state.counter_blur_offset = (state.counter_blur_offset + rng.gen_range(-2..3)) % 20;

        // AGGRESSIVE grid twinkling - add new random twinkle points
        for _ in 0..rng.gen_range(3..8) {
            let x = rng.gen_range(0..200) * 40; // Grid spacing
            let y = rng.gen_range(0..60) * 20;
            let intensity = rng.gen_range(150..255);
            state.grid_twinkle_map.push((x, y, intensity));
        }

        // Decay existing twinkles and remove dead ones
        state.grid_twinkle_map.retain_mut(|(_, _, intensity)| {
            *intensity = intensity.saturating_sub(15);
            *intensity > 0
        });

        // ULTRA-FAST ledger scrolling - new entry every ~100ms (every 3 frames at 30fps)
        if state.frame_count - last_log_frame >= 3 {
            last_log_frame = state.frame_count;

            let hash_prefix: u16 = rng.gen();
            let hash_suffix: u16 = rng.gen();
            let policy_num = 489 + (state.receipt_count % 100);

            // Calculate real timestamp (accelerated time)
            let total_seconds = (state.frame_count / 10) as u32; // Faster time passage
            let hours = 14 + (total_seconds / 3600) % 10;
            let minutes = (total_seconds / 60) % 60;
            let seconds = total_seconds % 60;

            let log_entry = format!(
                "[{:02}:{:02}:{:02}] 0x{:04x}...{:04x} | POL-{} | ACTIVE",
                hours, minutes, seconds, hash_prefix, hash_suffix, policy_num
            );

            // Insert at front (newest first)
            state.logs.insert(0, (log_entry, 0));

            // Keep only last 30 entries for longer scroll
            if state.logs.len() > 30 {
                state.logs.truncate(30);
            }
        }

        // Throttle to ~30 FPS (actually runs ~60fps for smoother animation)
        sleep(Duration::from_millis(16)).await;
    }
}

fn ui(f: &mut Frame, state: &mut AppState) {
    let area = f.size();

    // --- LAYER 1: ANIMATED GRID BACKGROUND ---
    {
        let buffer = f.buffer_mut();

        // Base grid (subtle, always present)
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                if x % 40 == 0 || y % 20 == 0 {
                    let cell = buffer.get_mut(x, y);
                    cell.set_char('·');
                    cell.set_fg(GLACIS_SLATE_DARK);
                }
            }
        }

        // Apply persistent twinkling points from state
        for (tx, ty, intensity) in state.grid_twinkle_map.iter() {
            let x = *tx;
            let y = *ty;
            if x < area.width && y < area.height {
                let cell = buffer.get_mut(x, y);

                // Create pulsing effect based on intensity
                let bright_factor = *intensity as f64 / 255.0;
                let twinkle_color = Color::Rgb(
                    (6.0 + bright_factor * 180.0) as u8,
                    (182.0 + bright_factor * 30.0) as u8,
                    (212.0 + bright_factor * 43.0) as u8,
                );

                cell.set_char('●');
                cell.set_fg(twinkle_color);
            }
        }

        // TRIPLE scanline sweep (more aggressive)
        for offset in 0..3 {
            let scan_y = ((state.scan_line_y as u16 * area.height) / 100 + offset).min(area.height - 1);
            for x in area.left()..area.right() {
                let cell = buffer.get_mut(x, scan_y);
                let current_fg = cell.fg;

                // Stronger scanline effect with cyan tint
                if let Color::Rgb(r, g, b) = current_fg {
                    let boost = (40 - (offset * 10)) as u8;
                    cell.set_fg(Color::Rgb(
                        r.saturating_add(boost / 2),
                        g.saturating_add(boost),
                        b.saturating_add(boost),
                    ));
                }
            }
        }
    }

    // --- LAYER 2: MAIN LAYOUT ---
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ])
        .split(main_layout[1]);

    // --- BREATHING HEADER (Smoother, faster sine-wave pulse) ---
    let pulse = ((state.frame_count as f64 * 0.08).sin() + 1.0) / 2.0; // 0.0 to 1.0, faster
    let secondary_pulse = ((state.frame_count as f64 * 0.12).cos() + 1.0) / 2.0;

    // Blend between cyan and bright white (more dramatic)
    let header_color = Color::Rgb(
        (6.0 + pulse * 240.0) as u8,        // Full spectrum from cyan to white
        (182.0 + pulse * 68.0 + secondary_pulse * 5.0) as u8,
        (212.0 + pulse * 43.0) as u8,
    );

    let header_text = "GLACIS // CHAIN OF CUSTODY // v0.1.0";
    let header = Paragraph::new(header_text)
        .style(Style::default()
            .fg(header_color)
            .add_modifier(Modifier::BOLD))
        .alignment(Alignment::Center)
        .block(Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(GLACIS_SLATE)));
    f.render_widget(header, main_layout[0]);

    // --- LEFT PANEL: LIVE SCROLLING LEDGER with AGGRESSIVE FADE ---
    let mut log_lines = Vec::new();
    for (log_text, age) in state.logs.iter() {
        // More dramatic fade: newest is cyan-white, old entries fade to nearly invisible
        let fade_factor = 1.0 - (*age as f64 / 80.0).min(0.95); // Faster fade

        // Newest items are bright cyan, fade to dark slate
        let log_color = if *age < 5 {
            // Super bright for newest entries
            Color::Rgb(
                (200.0 + fade_factor * 55.0) as u8,
                (240.0 + fade_factor * 15.0) as u8,
                (255.0) as u8,
            )
        } else {
            // Gradual fade for older entries
            Color::Rgb(
                (180.0 * fade_factor) as u8,
                (190.0 * fade_factor) as u8,
                (200.0 * fade_factor) as u8,
            )
        };

        log_lines.push(Line::from(vec![
            Span::styled(format!("  {}", log_text), Style::default().fg(log_color)),
        ]));
    }

    let ledger = Paragraph::new(log_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(" LEDGER EVENTS ", Style::default()
                .fg(GLACIS_CYAN)
                .add_modifier(Modifier::BOLD)))
            .border_style(Style::default().fg(GLACIS_SLATE)));
    f.render_widget(ledger, body_chunks[0]);

    // --- RIGHT PANEL: HYPER-FAST BLURRED COUNTER ---
    // Create a "motion blur" effect by showing multiple overlapping numbers
    let main_count = state.receipt_count;

    // Ghost numbers for blur effect (previous/next values)
    let ghost1 = main_count.saturating_sub(1);
    let ghost2 = main_count + 1;

    let stats_lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  RECEIPTS MINTED", Style::default().fg(GLACIS_SLATE)),
        ]),
        // Main counter with bright cyan
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{:08}", ghost1),
                Style::default().fg(Color::Rgb(6, 182, 212)).add_modifier(Modifier::DIM)
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{:08}", main_count),
                Style::default().fg(GLACIS_CYAN).add_modifier(Modifier::BOLD)
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{:08}", ghost2),
                Style::default().fg(Color::Rgb(6, 182, 212)).add_modifier(Modifier::DIM)
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  RISK SCORE", Style::default().fg(GLACIS_SLATE)),
        ]),
        Line::from(vec![
            Span::styled("  0.001%", Style::default().fg(GLACIS_SUCCESS)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  INSURANCE STATUS", Style::default().fg(GLACIS_SLATE)),
        ]),
        Line::from(vec![
            Span::styled("  ACTIVE", Style::default().fg(GLACIS_SUCCESS).add_modifier(Modifier::BOLD)),
        ]),
    ];

    let stats = Paragraph::new(stats_lines)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_type(ratatui::widgets::BorderType::Rounded)
            .title(Span::styled(" ACTUARIAL STATE ", Style::default()
                .fg(GLACIS_PURPLE)
                .add_modifier(Modifier::BOLD)))
            .border_style(Style::default().fg(GLACIS_SLATE)))
        .alignment(Alignment::Center);
    f.render_widget(stats, body_chunks[1]);

    // --- FOOTER: PULSING STATUS ---
    let status_pulse = if (state.frame_count / 30) % 2 == 0 {
        GLACIS_ALERT
    } else {
        GLACIS_SUCCESS
    };

    let footer_text = " ● SYSTEM SECURE  |  S3P SAMPLING: ON  |  ENCLAVE: LOCKED";
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(status_pulse))
        .block(Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(GLACIS_SLATE)));
    f.render_widget(footer, main_layout[2]);
}
