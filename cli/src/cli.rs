use crossbeam::channel::Receiver;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::error::Error;
use std::io;
use std::time::{Duration, Instant};
use tui::{
    backend::{Backend, CrosstermBackend},
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame, Terminal,
};
use unicode_width::UnicodeWidthStr;

use rusty_engine::core::log::LogMessage;
use rusty_engine::core::Control;

enum InputMode {
    Normal,
    Editing,
}

/// Temporary engine control mechanism.
pub struct CliControl {
    running: bool,
    log_recv: Receiver<LogMessage>,
    control: Control,
    log_data: Vec<String>,
    input: String,
    input_mode: InputMode,
    last_tick: Instant,
}

impl CliControl {
    pub fn new(log_recv: Receiver<LogMessage>, control: Control) -> Self {
        Self {
            running: false,
            log_recv,
            control,
            log_data: Vec::<String>::new(),
            input: String::new(),
            input_mode: InputMode::Normal,
            last_tick: Instant::now(),
        }
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.running = true;

        let mut terminal = setup_terminal()?;

        loop {
            self.flush_logs();

            if !self.running {
                println!("Exiting CLI Control");
                break;
            }
            terminal.draw(|rect| render(rect, self))?;

            self.poll_events()?;
        }

        // restore terminal
        resetore_terminal(terminal)?;

        Ok(())
    }

    fn flush_logs(&mut self) {
        if let Ok(msg) = self.log_recv.try_recv() {
            self.log_data.push(msg.message);
            let len = self.log_data.len();
            if len >= 30 {
                let new_log_data = self.log_data.split_off(len - 30);
                self.log_data = new_log_data;
            }
        }
    }

    fn poll_events(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let tick_rate = Duration::from_millis(20);
        let timeout = tick_rate
            .checked_sub(self.last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_secs(0));
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                match self.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char(':') => {
                            self.input_mode = InputMode::Editing;
                        }
                        KeyCode::Char('s') => {
                            self.control.start();
                        }
                        KeyCode::Char('p') => {
                            self.control.pause();
                        }
                        KeyCode::Char('u') => {
                            self.control.unpause();
                        }
                        KeyCode::Char('q') => {
                            self.stop();
                            return Ok(false);
                        }
                        _ => {}
                    },
                    InputMode::Editing => match key.code {
                        KeyCode::Enter => {
                            let cmd_str = std::mem::take(&mut self.input);
                            match cmd_str.as_str() {
                                "stop" => self.stop(),
                                "start" => self.control.start(),
                                "pause" => self.control.pause(),
                                "unpause" => self.control.unpause(),
                                "clear" => self.log_data.clear(),
                                "exit" => self.stop(),
                                _ => (),
                            }
                        }
                        KeyCode::Char(c) => {
                            self.input.push(c);
                        }
                        KeyCode::Backspace => {
                            self.input.pop();
                        }
                        KeyCode::Esc => {
                            self.input_mode = InputMode::Normal;
                        }
                        _ => {}
                    },
                }
            }
        }
        if self.last_tick.elapsed() >= tick_rate {
            self.last_tick = Instant::now();
        }
        Ok(true)
    }

    fn stop(&mut self) {
        self.running = false;
        self.control.stop();
    }
}

fn resetore_terminal(
    mut terminal: Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn Error + 'static>> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn render<B: Backend>(rect: &mut Frame<B>, cli: &CliControl) {
    let size = rect.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        // .margin(1)
        .constraints(
            [
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(size);

    render_status(rect, chunks[0]);

    render_output(rect, cli, chunks[1]);

    render_input(rect, cli, chunks[2]);
}

fn render_status<B: Backend>(rect: &mut Frame<'_, B>, chunk: tui::layout::Rect) {
    let status_block = Paragraph::new(format!(
        "Engine Status: {:?}",
        // self.internal.borrow().controllable.state()
        "Unknown"
    ))
    .style(Style::default().fg(Color::LightCyan))
    .alignment(Alignment::Left)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::White))
            .title("Engine Status")
            .border_type(BorderType::Plain),
    );
    rect.render_widget(status_block, chunk);
}

fn render_output<B: Backend>(rect: &mut Frame<'_, B>, cli: &CliControl, chunk: tui::layout::Rect) {
    let mut output = String::new();
    for msg in cli.log_data.iter() {
        output.push_str(msg);
        output.push_str("\r\n");
    }
    let output_block = Paragraph::new(output.clone())
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::White))
                .title("Output")
                .border_type(BorderType::Plain),
        );

    rect.render_widget(output_block, chunk);
}

fn render_input<B: Backend>(rect: &mut Frame<'_, B>, cli: &CliControl, chunk: tui::layout::Rect) {
    match cli.input_mode {
        InputMode::Editing => {
            let input_block = Paragraph::new(cli.input.clone())
                .style(Style::default().fg(Color::LightCyan))
                .alignment(Alignment::Left)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .style(Style::default().fg(Color::White))
                        .title("Input")
                        .border_type(BorderType::Plain),
                );
            rect.render_widget(input_block, chunk);
            rect.set_cursor(
                // Put cursor past the end of the input text
                chunk.x + cli.input.width() as u16 + 1,
                // Move one line down, from the border to the input line
                chunk.y + 1,
            )
        }
        InputMode::Normal => {
            let help_block = Paragraph::new("Press ':' to enter command mode. 's' to start, 'p' pause, 'u' to unpause, 'q' to quit.")
                .style(Style::default().fg(Color::LightCyan))
                .alignment(Alignment::Left)
                .block(Block::default().borders(Borders::ALL));
            rect.render_widget(help_block, chunk);
        }
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>, Box<dyn Error + 'static>> {
    enable_raw_mode().expect("can run in raw mode");
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;
    Ok(terminal)
}
