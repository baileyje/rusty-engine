use std::sync::{mpsc::Receiver, Arc, Mutex};
use termwiz::{
  caps::Capabilities,
  cell::AttributeChange,
  color::{AnsiColor, ColorAttribute, RgbColor},
  input::{InputEvent, KeyCode, KeyEvent},
  surface::Change,
  terminal::{buffered::BufferedTerminal, SystemTerminal, Terminal},
  widgets::{
    layout::{ChildOrientation, Constraints},
    CursorShapeAndPosition, RenderArgs, Ui, UpdateArgs, Widget, WidgetEvent,
  },
  Error,
};

use super::{
  control::{Control, Controllable},
  logger::LogMessage,
};

struct Internal {
  running: bool,
  controllable: Box<dyn Controllable>,
  log_data: Vec<String>,
}

impl Internal {
  pub fn new(controllable: Box<dyn Controllable>) -> Self {
    Self {
      running: true,
      controllable: controllable,
      log_data: Vec::<String>::new(),
    }
  }

  pub fn handle_command(&mut self, command: String) {
    if command == "stop" {
      self.controllable.stop().unwrap();
    }
    if command == "exit" {
      self.running = false;
    }
    if command == "start" {
      self.controllable.start().unwrap();
    }
    if command == "pause" {
      self.controllable.pause().unwrap();
    }
    if command == "unpause" {
      self.controllable.unpause().unwrap();
    }
  }
}

/// Temporary engine control mechanism.
pub struct CliControl<'a> {
  internal: Arc<Mutex<Internal>>,
  ui: Ui<'a>,
  buff: BufferedTerminal<SystemTerminal>,
  log_recv: Receiver<LogMessage>,
}

impl<'a> Control for CliControl<'a> {
  // Start the control system. Listen for commands until we see `stop`
  fn start(&mut self) {
    self.run().unwrap();
  }
}

impl<'a> CliControl<'a> {
  pub fn new(controllable: Box<dyn Controllable>, log_recv: Receiver<LogMessage>) -> Self {
    let caps = Capabilities::new_from_env().expect("Unable to get capabilities");
    let terminal = SystemTerminal::new(caps).expect("Could not get terminal");

    let mut buff = BufferedTerminal::new(terminal).expect("Unable to get buffered terminal");
    buff.terminal().set_raw_mode().unwrap();

    let mut instance = Self {
      ui: Ui::new(),
      buff,
      internal: Arc::new(Mutex::new(Internal::new(controllable))),
      log_recv,
    };

    let root_id = instance.ui.set_root(MainScreen::new());
    let log = LogData::new(Arc::clone(&instance.internal));
    instance.ui.add_child(root_id, log);
    let input = CommandInput::new(Arc::clone(&instance.internal));
    let buffer_id = instance.ui.add_child(root_id, input);
    let status_line = StatusLine::new(Arc::clone(&instance.internal));
    instance.ui.add_child(root_id, status_line);
    instance.ui.set_focus(buffer_id);

    instance
  }

  pub fn run(&mut self) -> Result<(), Error> {
    loop {
      // Flush log and ensure still running
      {
        let mut internal = self.internal.lock().unwrap();
        if let Ok(msg) = self.log_recv.try_recv() {
          // println!("Message: {:?}", msg);
          internal.log_data.push(msg.message);
        }
        if !internal.running {
          return Ok(());
        }
      }
      self.ui.process_event_queue()?;
      if self.ui.render_to_screen(&mut self.buff)? {
        continue;
      }
      self.buff.flush()?;
      // Wait for user input
      match self
        .buff
        .terminal()
        .poll_input(Some(std::time::Duration::new(0, 0)))
      {
        Ok(Some(InputEvent::Resized { rows, cols })) => {
          // FIXME: this is working around a bug where we don't realize
          // that we should redraw everything on resize in BufferedTerminal.
          self
            .buff
            .add_change(Change::ClearScreen(Default::default()));
          self.buff.resize(cols, rows);
        }
        Ok(Some(input)) => match input {
          InputEvent::Key(KeyEvent {
            key: KeyCode::Escape,
            ..
          }) => {
            self
              .buff
              .add_change(Change::ClearScreen(Default::default()));
            self.buff.flush()?;
            self.internal.lock().unwrap().controllable.stop().unwrap();
            break;
          }
          input @ _ => {
            self.ui.queue_event(WidgetEvent::Input(input));
          }
        },
        Ok(None) => {}
        Err(e) => {
          print!("{:?}\r\n", e);
          break;
        }
      }
    }
    Ok(())
  }
}

struct MainScreen {}

impl MainScreen {
  pub fn new() -> Self {
    Self {}
  }
}

impl Widget for MainScreen {
  fn render(&mut self, args: &mut RenderArgs) {
    args
      .surface
      .add_change(Change::ClearScreen(AnsiColor::White.into()));
  }

  fn get_size_constraints(&self) -> Constraints {
    // Switch from default horizontal layout to vertical layout
    let mut c = Constraints::default();
    c.child_orientation = ChildOrientation::Vertical;
    c
  }
}

/// This is the main text input area for the app
struct LogData {
  control: Arc<Mutex<Internal>>,
}

impl LogData {
  pub fn new(control: Arc<Mutex<Internal>>) -> Self {
    Self { control }
  }
}

impl Widget for LogData {
  /// Draw ourselves into the surface provided by RenderArgs
  fn render(&mut self, args: &mut RenderArgs) {
    args
      .surface
      .add_change(Change::ClearScreen(AnsiColor::Black.into()));
    args
      .surface
      .add_change(&self.control.lock().unwrap().log_data.join("\r\n"));
  }

  fn get_size_constraints(&self) -> Constraints {
    let c = Constraints::default();
    c
  }
}

struct CommandInput {
  text: String,
  control: Arc<Mutex<Internal>>,
}

impl CommandInput {
  /// Initialize the widget with the input text
  pub fn new(control: Arc<Mutex<Internal>>) -> Self {
    Self {
      text: String::new(),
      control,
    }
  }
}

impl Widget for CommandInput {
  fn process_event(&mut self, event: &WidgetEvent, _args: &mut UpdateArgs) -> bool {
    match event {
      WidgetEvent::Input(InputEvent::Key(KeyEvent {
        key: KeyCode::Backspace,
        ..
      })) => {
        if self.text.len() > 0 {
          self.text.remove(self.text.len() - 1);
        }
      }
      WidgetEvent::Input(InputEvent::Key(KeyEvent {
        key: KeyCode::Char(c),
        ..
      })) => self.text.push(*c),
      WidgetEvent::Input(InputEvent::Key(KeyEvent {
        key: KeyCode::Enter,
        ..
      })) => {
        self
          .control
          .lock()
          .unwrap()
          .handle_command(self.text.clone());
        self.text.clear();
      }
      WidgetEvent::Input(InputEvent::Paste(s)) => {
        self.text.push_str(&s);
      }
      _ => {}
    }

    true // handled it all
  }

  /// Draw ourselves into the surface provided by RenderArgs
  fn render(&mut self, args: &mut RenderArgs) {
    args.surface.add_change(Change::ClearScreen(
      ColorAttribute::TrueColorWithPaletteFallback(
        RgbColor::new(0x31, 0x1B, 0x92),
        AnsiColor::Black.into(),
      ),
    ));
    args
      .surface
      .add_change(Change::Attribute(AttributeChange::Foreground(
        ColorAttribute::TrueColorWithPaletteFallback(
          RgbColor::new(0xB3, 0x88, 0xFF),
          AnsiColor::Purple.into(),
        ),
      )));
    args.surface.add_change(format!("> {}", self.text.clone()));

    // Place the cursor at the end of the text.
    // A more advanced text editing widget would manage the
    // cursor position differently.
    *args.cursor = CursorShapeAndPosition {
      coords: args.surface.cursor_position().into(),
      shape: termwiz::surface::CursorShape::SteadyBar,
      ..Default::default()
    };
  }

  fn get_size_constraints(&self) -> Constraints {
    let mut c = Constraints::default();
    c.set_fixed_height(1);
    c
  }
}

// This is a little status line widget that we render at the bottom
struct StatusLine {
  control: Arc<Mutex<Internal>>,
}

impl StatusLine {
  pub fn new(control: Arc<Mutex<Internal>>) -> Self {
    Self { control }
  }
}

impl Widget for StatusLine {
  /// Draw ourselves into the surface provided by RenderArgs
  fn render(&mut self, args: &mut RenderArgs) {
    args
      .surface
      .add_change(Change::ClearScreen(AnsiColor::Grey.into()));
    let state = self.control.lock().unwrap().controllable.state();
    args
      .surface
      .add_change(format!("Engine Status: {:?}", state));
  }

  fn get_size_constraints(&self) -> Constraints {
    let mut c = Constraints::default();
    c.set_fixed_height(1);
    c
  }
}
