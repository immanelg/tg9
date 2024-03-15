use anyhow::Result;

use futures::{future::FutureExt, StreamExt};

use crossterm::{
    cursor,
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyEvent, KeyEventKind, MouseEvent,
    },
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend as Backend;
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
    task::JoinHandle,
};

#[derive(Clone, Debug)]
pub enum TermEvent {
    Init,
    Quit,
    Error,
    Closed,
    Tick,
    Render,
    FocusGained,
    FocusLost,
    Paste(String),
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
}

pub struct TermApi {
    pub terminal: ratatui::Terminal<Backend<std::io::Stderr>>,
    pub task: JoinHandle<()>,
    pub event_rx: UnboundedReceiver<TermEvent>,
    pub event_tx: UnboundedSender<TermEvent>,
    pub mouse: bool,
    pub paste: bool,
}

impl TermApi {
    pub fn new() -> Result<Self> {
        let terminal = ratatui::Terminal::new(Backend::new(std::io::stderr()))?;
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(async {});
        let mouse = true;
        let paste = true;
        Ok(Self {
            terminal,
            task,
            event_rx,
            event_tx,
            mouse,
            paste,
        })
    }

    pub fn start(&mut self) {
        let tick_delay = std::time::Duration::from_secs_f64(1.0 / 4.0);
        let render_delay = std::time::Duration::from_secs_f64(1.0 / 60.0);
        let event_tx = self.event_tx.clone();
        self.task = tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_delay);
            let mut render_interval = tokio::time::interval(render_delay);
            // event_tx.send(TermEvent::Init).unwrap();
            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
                  maybe_event = crossterm_event => {
                    match maybe_event {
                      Some(Ok(evt)) => {
                        match evt {
                          Event::Key(key) => {
                            if key.kind == KeyEventKind::Press {
                              event_tx.send(TermEvent::Key(key)).unwrap();
                            }
                          },
                          Event::Mouse(mouse) => {
                            event_tx.send(TermEvent::Mouse(mouse)).unwrap();
                          },
                          Event::Resize(x, y) => {
                            event_tx.send(TermEvent::Resize(x, y)).unwrap();
                          },
                          Event::FocusLost => {
                            event_tx.send(TermEvent::FocusLost).unwrap();
                          },
                          Event::FocusGained => {
                            event_tx.send(TermEvent::FocusGained).unwrap();
                          },
                          Event::Paste(s) => {
                            event_tx.send(TermEvent::Paste(s)).unwrap();
                          },
                        }
                      }
                      Some(Err(_)) => {
                        event_tx.send(TermEvent::Error).unwrap();
                      }
                      None => {},
                    }
                  },
                  _ = tick_delay => {
                      event_tx.send(TermEvent::Tick).unwrap();
                  },
                  _ = render_delay => {
                      event_tx.send(TermEvent::Render).unwrap();
                  },
                }
            }
        });
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
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.terminal.flush()?;
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

    pub fn suspend(&mut self) -> Result<()> {
        self.exit()?;
        #[cfg(not(windows))]
        signal_hook::low_level::raise(signal_hook::consts::signal::SIGTSTP)?;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<()> {
        self.enter()?;
        Ok(())
    }

    pub async fn next(&mut self) -> Option<TermEvent> {
        self.event_rx.recv().await
    }
}

