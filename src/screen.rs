use anyhow::Result;

use futures::{future::FutureExt, StreamExt};

use std::io::{stdout, Stdout};
use crossterm;
use crossterm::{
    cursor::{Hide, Show},
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyEvent, KeyEventKind, MouseEvent,
    },
    terminal::{disable_raw_mode, is_raw_mode_enabled, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend as Backend;
use tokio::{
    sync::mpsc,
    task::JoinHandle,
};

#[derive(Clone, Debug)]
pub enum ScreenEvent {
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

pub struct Screen {
    pub terminal: ratatui::Terminal<Backend<Stdout>>,
    pub task: JoinHandle<()>,
    pub tx: mpsc::UnboundedSender<ScreenEvent>,
    pub mouse: bool,
    pub paste: bool,
}

impl Screen {
    pub fn new(tx: mpsc::UnboundedSender<ScreenEvent>) -> Result<Self> {
        let terminal = ratatui::Terminal::new(Backend::new(stdout()))?;
        let task = tokio::spawn(async {});
        let mouse = true;
        let paste = true;
        Ok(Self {
            terminal,
            task,
            tx,
            mouse,
            paste,
        })
    }

    pub fn start(&mut self) {
        let tick_delay = std::time::Duration::from_secs_f64(1.0);
        let render_delay = std::time::Duration::from_secs_f64(1.0 / 20.0);
        let event_tx = self.tx.clone();
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
                    // TODO: signals
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    Event::Key(key) => {
                                        if key.kind == KeyEventKind::Press {
                                            event_tx.send(ScreenEvent::Key(key)).unwrap();
                                        }
                                    },
                                    Event::Mouse(mouse) => {
                                        event_tx.send(ScreenEvent::Mouse(mouse)).unwrap();
                                    },
                                    Event::Resize(x, y) => {
                                        event_tx.send(ScreenEvent::Resize(x, y)).unwrap();
                                    },
                                    Event::FocusLost => {
                                        event_tx.send(ScreenEvent::FocusLost).unwrap();
                                    },
                                    Event::FocusGained => {
                                        event_tx.send(ScreenEvent::FocusGained).unwrap();
                                    },
                                    Event::Paste(s) => {
                                        event_tx.send(ScreenEvent::Paste(s)).unwrap();
                                    },
                                }
                            }
                            Some(Err(_)) => {
                                event_tx.send(ScreenEvent::Error).unwrap();
                            }
                            None => {},
                        }
                    },
                        _ = tick_delay => {
                            event_tx.send(ScreenEvent::Tick).unwrap();
                        },
                        _ = render_delay => {
                        event_tx.send(ScreenEvent::Render).unwrap();
                    },
                }
            }
        });
    }

    pub fn enter(&mut self) -> Result<()> {
        enable_raw_mode()?;
        crossterm::execute!(stdout(), EnterAlternateScreen, Hide, EnableMouseCapture, EnableBracketedPaste)?;
        self.start();
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        if is_raw_mode_enabled()? {
            self.terminal.flush()?;
            crossterm::execute!(stdout(), LeaveAlternateScreen, Show, DisableMouseCapture, DisableBracketedPaste)?;
            disable_raw_mode()?;
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
    //
    // pub async fn next(&mut self) -> Option<ScreenEvent> {
    //     self.rx.recv().await
    // }
}

pub fn setup_panic_handler() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        crossterm::execute!(std::io::stdout(), LeaveAlternateScreen, DisableBracketedPaste, DisableMouseCapture).unwrap();
        disable_raw_mode().unwrap();
        original_hook(panic_info);
    }));
}
