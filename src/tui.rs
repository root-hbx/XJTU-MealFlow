use std::{
    io::{Stderr, stderr},
    ops::{Deref, DerefMut},
    time::Duration,
};

use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use futures::{FutureExt, StreamExt};
use ratatui::{
    Frame,
    backend::{CrosstermBackend, TestBackend},
    crossterm::{
        cursor,
        event::{
            DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
            Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent,
        },
        terminal::{EnterAlternateScreen, LeaveAlternateScreen},
    },
};
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug)]
pub enum Event {
    Init,
    // Quit,
    Error,
    // Closed,
    Tick,
    Render,
    FocusGained,
    FocusLost,
    Paste(String),
    Key(KeyEvent),
    #[allow(dead_code)]
    Mouse(MouseEvent),
    #[allow(dead_code)]
    Resize(u16, u16),
}

impl From<KeyCode> for Event {
    fn from(value: KeyCode) -> Self {
        Event::Key(KeyEvent::new(value, KeyModifiers::NONE))
    }
}
impl From<char> for Event {
    fn from(value: char) -> Self {
        Event::Key(KeyEvent::new(KeyCode::Char(value), KeyModifiers::NONE))
    }
}

pub enum TuiEnum {
    Crossterm(Tui),
    Test(TestTui),
}

impl From<Tui> for TuiEnum {
    fn from(tui: Tui) -> Self {
        TuiEnum::Crossterm(tui)
    }
}
impl From<TestTui> for TuiEnum {
    fn from(tui: TestTui) -> Self {
        TuiEnum::Test(tui)
    }
}
impl TuiEnum {
    pub fn enter(&mut self) -> Result<()> {
        match self {
            TuiEnum::Crossterm(tui) => tui.enter(),
            TuiEnum::Test(_) => Ok(()),
        }
    }
    pub fn exit(&mut self) -> Result<()> {
        match self {
            TuiEnum::Crossterm(tui) => tui.exit(),
            TuiEnum::Test(_) => Ok(()),
        }
    }
    pub async fn next(&mut self) -> Result<Event> {
        match self {
            TuiEnum::Crossterm(tui) => tui.next().await,
            TuiEnum::Test(_) => Ok(Event::Tick),
        }
    }
    pub fn draw(&mut self, f: impl FnOnce(&mut Frame)) -> Result<()> {
        match self {
            TuiEnum::Crossterm(tui) => tui.draw(f).map(|_| ()).map_err(Into::into),
            TuiEnum::Test(tui) => tui.draw(f).map(|_| ()).map_err(Into::into),
        }
    }
}

pub struct Tui {
    pub terminal: ratatui::Terminal<CrosstermBackend<Stderr>>,
    pub task: JoinHandle<()>,
    pub cancellation_token: CancellationToken,
    pub event_rx: UnboundedReceiver<Event>,
    pub event_tx: UnboundedSender<Event>,
    pub frame_rate: f64,
    pub tick_rate: f64,
    pub mouse: bool,
    pub paste: bool,
}

impl Tui {
    pub fn new() -> Result<Self> {
        let tick_rate = 4.0;
        let frame_rate = 60.0;
        let terminal = ratatui::Terminal::new(CrosstermBackend::new(stderr()))?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let cancellation_token = CancellationToken::new();
        let task = tokio::spawn(async {});
        let mouse = false;
        let paste = false;
        Ok(Self {
            terminal,
            task,
            cancellation_token,
            event_rx,
            event_tx,
            frame_rate,
            tick_rate,
            mouse,
            paste,
        })
    }

    pub fn tick_rate(mut self, tick_rate: f64) -> Self {
        self.tick_rate = tick_rate;
        self
    }

    pub fn frame_rate(mut self, frame_rate: f64) -> Self {
        self.frame_rate = frame_rate;
        self
    }

    #[allow(dead_code)]
    pub fn mouse(mut self, mouse: bool) -> Self {
        self.mouse = mouse;
        self
    }

    #[allow(dead_code)]
    pub fn paste(mut self, paste: bool) -> Self {
        self.paste = paste;
        self
    }

    pub fn start(&mut self) {
        let tick_delay = std::time::Duration::from_secs_f64(1.0 / self.tick_rate);
        let render_delay = std::time::Duration::from_secs_f64(1.0 / self.frame_rate);
        self.cancel();
        self.cancellation_token = CancellationToken::new();
        let _cancellation_token = self.cancellation_token.clone();
        let _event_tx = self.event_tx.clone();
        self.task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);
            _event_tx.send(Event::Init).unwrap();
            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
                  _ = _cancellation_token.cancelled() => {
                    break;
                  }
                  maybe_event = crossterm_event => {
                    match maybe_event {
                      Some(Ok(evt)) => {
                        match evt {
                          CrosstermEvent::Key(key) => {
                            if key.kind == KeyEventKind::Press {
                              _event_tx.send(Event::Key(key)).unwrap();
                            }
                          },
                          CrosstermEvent::Mouse(mouse) => {
                            _event_tx.send(Event::Mouse(mouse)).unwrap();
                          },
                          CrosstermEvent::Resize(x, y) => {
                            _event_tx.send(Event::Resize(x, y)).unwrap();
                          },
                          CrosstermEvent::FocusLost => {
                            _event_tx.send(Event::FocusLost).unwrap();
                          },
                          CrosstermEvent::FocusGained => {
                            _event_tx.send(Event::FocusGained).unwrap();
                          },
                          CrosstermEvent::Paste(s) => {
                            _event_tx.send(Event::Paste(s)).unwrap();
                          }
                        }
                      }
                      Some(Err(_)) => {
                        _event_tx.send(Event::Error).unwrap();
                      }
                      None => {},
                    }
                  },
                  _ = tick_delay => {
                      _event_tx.send(Event::Tick).unwrap();
                  },
                  _ = render_delay => {
                      _event_tx.send(Event::Render).unwrap();
                  },
                }
            }
        });
    }

    pub fn stop(&self) -> Result<()> {
        self.cancel();
        let mut counter = 0;
        while !self.task.is_finished() {
            std::thread::sleep(Duration::from_millis(1));
            counter += 1;
            if counter > 50 {
                self.task.abort();
            }
            if counter > 100 {
                tracing::error!("Failed to abort task in 100 milliseconds for unknown reason");
                break;
            }
        }
        Ok(())
    }

    pub fn enter(&mut self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stderr(), EnterAlternateScreen, cursor::Hide)?;
        if self.mouse {
            crossterm::execute!(std::io::stderr(), EnableMouseCapture)?;
        }
        if self.paste {
            crossterm::execute!(std::io::stderr(), EnableBracketedPaste)?;
        }
        self.start();
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        self.stop()?;
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.flush()?;
            if self.paste {
                crossterm::execute!(std::io::stderr(), DisableBracketedPaste)?;
            }
            if self.mouse {
                crossterm::execute!(std::io::stderr(), DisableMouseCapture)?;
            }
            crossterm::execute!(std::io::stderr(), LeaveAlternateScreen, cursor::Show)?;
            crossterm::terminal::disable_raw_mode()?;
        }
        Ok(())
    }

    pub fn cancel(&self) {
        self.cancellation_token.cancel();
    }

    #[allow(dead_code)]
    pub fn suspend(&mut self) -> Result<()> {
        self.exit()?;
        #[cfg(not(windows))]
        signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn resume(&mut self) -> Result<()> {
        self.enter()?;
        Ok(())
    }

    pub async fn next(&mut self) -> Result<Event> {
        self.event_rx
            .recv()
            .await
            .ok_or(color_eyre::eyre::eyre!("Unable to get event"))
    }
}

impl Deref for Tui {
    type Target = ratatui::Terminal<CrosstermBackend<Stderr>>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for Tui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.exit().unwrap();
    }
}

pub struct TestTui {
    pub terminal: ratatui::Terminal<TestBackend>,
}

impl TestTui {
    #[cfg(test)]
    pub fn new() -> Self {
        let terminal = ratatui::Terminal::new(TestBackend::new(80, 25)).unwrap();
        Self { terminal }
    }
}

impl Deref for TestTui {
    type Target = ratatui::Terminal<TestBackend>;

    fn deref(&self) -> &Self::Target {
        &self.terminal
    }
}

impl DerefMut for TestTui {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.terminal
    }
}

#[cfg(test)]
impl TuiEnum {
    pub fn backend(&self) -> &TestBackend {
        match self {
            TuiEnum::Crossterm(_) => panic!("Not a test backend"),
            TuiEnum::Test(tui) => tui.backend(),
        }
    }
}
