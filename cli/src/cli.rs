use std::sync::{mpsc::Receiver};
use std::cell::RefCell;
use std::rc::Rc;
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

use rusty_engine::core::{
  control::{Control, EngineControl},
  logger::LogMessage
};

struct Internal {
  running: bool,
  controllable: Box<dyn EngineControl>,
  log_data: Vec<String>,
}

impl Internal {
  pub fn new(controllable: Box<dyn EngineControl>) -> Self {
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
  internal: Rc<RefCell<Internal>>,
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
  pub fn new(controllable: Box<dyn EngineControl>, log_recv: Receiver<LogMessage>) -> Self {
    let caps = Capabilities::new_from_env().expect("Unable to get capabilities");
    let terminal = SystemTerminal::new(caps).expect("Could not get terminal");

    let mut buff = BufferedTerminal::new(terminal).expect("Unable to get buffered terminal");
    buff.terminal().set_raw_mode().unwrap();
    
    let internal = Rc::new(RefCell::new(Internal::new(controllable)));

    let mut ui = Ui::new();

    let root_id = ui.set_root(MainScreen::new());
    let log = LogData::new(Rc::clone(&internal));
    ui.add_child(root_id, log);
    let input = CommandInput::new(Rc::clone(&internal));
    let buffer_id = ui.add_child(root_id, input);
    let status_line = StatusLine::new(Rc::clone(&internal));
    ui.add_child(root_id, status_line);
    ui.set_focus(buffer_id);

    let instance = Self {
      ui,
      buff,
      internal,
      log_recv
    };
    
    instance
  }

  pub fn run(&mut self) -> Result<(), Error> {
    loop {
      // Flush log and ensure still running
      {
        // Flush any log data out of the controllable
        self.internal.borrow().controllable.flush();
        if let Ok(msg) = self.log_recv.try_recv() {
          self.internal.borrow_mut().log_data.push(msg.message);
          let len = self.internal.borrow().log_data.len();
          if len >= 30 {
            let new_log_data = self.internal.borrow_mut().log_data.split_off(len - 30);
            self.internal.borrow_mut().log_data = new_log_data;
          }
        }
        if !self.internal.borrow().running {
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
            self.internal.borrow_mut().controllable.stop().unwrap();
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
  control: Rc<RefCell<Internal>>,
}

impl LogData {
  pub fn new(control: Rc<RefCell<Internal>>) -> Self {
    Self { control }
  }
}

impl Widget for LogData {
  /// Draw ourselves into the surface provided by RenderArgs
  fn render(&mut self, args: &mut RenderArgs) {
    // TODO: Summarize the data....
    let mut log_out = String::new();
    let len = self.control.borrow().log_data.len();
    let to_skip = 0;
    // if len > 30 {
    //   to_skip = len - 10;
    // }
    // let to_log = control.log_data[std::cmp::max(0, len - 10)..];
    for msg in self.control.borrow().log_data.iter().skip(to_skip) {
      log_out.push_str(msg);
      log_out.push_str("\r\n");
    }
    log_out.push_str(format!("{}\r\n", len).as_str());
    args
      .surface
      .add_change(Change::ClearScreen(AnsiColor::Black.into()));
    args
      .surface
      .add_change(log_out);
  }

  fn get_size_constraints(&self) -> Constraints {
    let c = Constraints::default();
    c
  }
}

struct CommandInput {
  text: String,
  control: Rc<RefCell<Internal>>,
}

impl CommandInput {
  /// Initialize the widget with the input text
  pub fn new(control: Rc<RefCell<Internal>>) -> Self {
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
          .borrow_mut()
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
  control: Rc<RefCell<Internal>>,
}

impl StatusLine {
  pub fn new(control: Rc<RefCell<Internal>>) -> Self {
    Self { control }
  }
}

impl Widget for StatusLine {
  /// Draw ourselves into the surface provided by RenderArgs
  fn render(&mut self, args: &mut RenderArgs) {
    args
      .surface
      .add_change(Change::ClearScreen(AnsiColor::Grey.into()));
    args
      .surface
      .add_change(format!("Engine Status: {:?}", self.control.borrow().controllable.state()));
  }

  fn get_size_constraints(&self) -> Constraints {
    let mut c = Constraints::default();
    c.set_fixed_height(1);
    c
  }
}
