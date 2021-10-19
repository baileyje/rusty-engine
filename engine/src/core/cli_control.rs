use termwiz::caps::Capabilities;
use termwiz::cell::AttributeChange;
use termwiz::color::{AnsiColor, ColorAttribute, RgbColor};
use termwiz::input::*;
use termwiz::surface::Change;
use termwiz::terminal::buffered::BufferedTerminal;
use termwiz::terminal::{SystemTerminal, Terminal};
use termwiz::widgets::layout::{ChildOrientation, Constraints};
use termwiz::widgets::*;
use termwiz::Error;

use std::sync::{Arc, Mutex};

use super::control::{Control, Controllable};

struct Internal {
  running: bool,
  controllable: Box<dyn Controllable>,
}

impl Internal {
  pub fn new(controllable: Box<dyn Controllable>) -> Self {
    Self {
      running: true,
      controllable: controllable,
    }
  }

  pub fn handle_command(&mut self, command: String) {
    println!("Handle command: {}", command);
    if command == "stop" {
      self.controllable.stop().unwrap();
      self.running = false;
    }
    if command == "start" {
      self.controllable.start().unwrap();
    }
    if command == "pause" {
      self.controllable.pause().unwrap();
    }
  }
}

/// Temporary engine control mechanism.
pub struct CliControl<'a> {
  internal: Arc<Mutex<Internal>>,
  ui: Ui<'a>,
  buff: BufferedTerminal<SystemTerminal>,
}

impl<'a> Control for CliControl<'a> {
  // fn on_state_change(&mut self, new_state: State) {
  //   self.internal.lock().unwrap().state = new_state;
  // }
  // fn on_event(&mut self, _: std::string::String) {
  //   todo!()
  // }

  // Start the control system. Listen for commands until we see `stop`
  fn start(&mut self) {
    // {
    //   self.internal.lock().unwrap().controllable.start().expect("Could not start controllable");
    // }
    self.run().unwrap();
  }
}

impl<'a> CliControl<'a> {
  pub fn new(controllable: Box<dyn Controllable>) -> Self {
    let caps = Capabilities::new_from_env().expect("Unable to get capabilities");
    let terminal = SystemTerminal::new(caps).expect("Could not get terminal");

    let mut buff = BufferedTerminal::new(terminal).expect("Unable to get buffered terminal");
    buff.terminal().set_raw_mode().unwrap();

    let mut instance = Self {
      ui: Ui::new(),
      buff,
      internal: Arc::new(Mutex::new(Internal::new(controllable))),
    };

    let root_id = instance.ui.set_root(MainScreen::new());
    // let log = LogData::new(&log_data);
    // ui.add_child(root_id, log);
    let input = CommandInput::new(Arc::clone(&instance.internal));
    let buffer_id = instance.ui.add_child(root_id, input);
    
    let status_line = StatusLine::new(Arc::clone(&instance.internal));
    instance.ui.add_child(root_id, status_line);
    instance.ui.set_focus(buffer_id);

    instance
  }


  pub fn run(&mut self) -> Result<(), Error> {
    println!("CLI Run");
    loop {
      if !self.internal.lock().unwrap().running {
        return Ok(())
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
            self.internal.lock().unwrap().handle_command(String::from("stop"));
            break;
          }
          input @ _ => {
            self.ui.queue_event(WidgetEvent::Input(input));
          }
        },
        Ok(None) => {
        }
        Err(e) => {
          print!("{:?}\r\n", e);
          break;
        }
      }
      // }
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
struct LogData<'a> {
  /// Holds the input text that we wish the widget to display
  text: &'a String,
}

impl<'a> LogData<'a> {
  /// Initialize the widget with the input text
  pub fn new(text: &'a String) -> Self {
    Self { text }
  }
}

impl<'a> Widget for LogData<'a> {
  /// Draw ourselves into the surface provided by RenderArgs
  fn render(&mut self, args: &mut RenderArgs) {
    args
      .surface
      .add_change(Change::ClearScreen(AnsiColor::Black.into()));
    args.surface.add_change(self.text);
  }

  fn get_size_constraints(&self) -> layout::Constraints {
    let c = layout::Constraints::default();
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
        key: KeyCode::Char(c),
        ..
      })) => self.text.push(*c),
      WidgetEvent::Input(InputEvent::Key(KeyEvent {
        key: KeyCode::Enter,
        ..
      })) => {
        self.control.lock().unwrap().handle_command(self.text.clone());
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
    args.surface.add_change(self.text.clone());

    // Place the cursor at the end of the text.
    // A more advanced text editing widget would manage the
    // cursor position differently.
    *args.cursor = CursorShapeAndPosition {
      coords: args.surface.cursor_position().into(),
      shape: termwiz::surface::CursorShape::SteadyBar,
      ..Default::default()
    };
  }

  fn get_size_constraints(&self) -> layout::Constraints {
    let mut c = layout::Constraints::default();
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

  fn get_size_constraints(&self) -> layout::Constraints {
    let mut c = layout::Constraints::default();
    c.set_fixed_height(1);
    c
  }
}
