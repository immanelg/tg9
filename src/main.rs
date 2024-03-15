mod screen;
mod api;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use grammers_client::types::iter_buffer::InvocationError;
use grammers_client::types::{Dialog, Message, Chat, MessageDeletion};
use grammers_client::{Client, Update};
use ratatui::{prelude::*, widgets::*};

use tokio::sync::mpsc::{self, UnboundedSender, UnboundedReceiver};
use screen::ScreenEvent;

pub fn setup_panic_handler() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        crossterm::execute!(std::io::stderr(), crossterm::terminal::LeaveAlternateScreen).unwrap();
        crossterm::terminal::disable_raw_mode().unwrap();
        original_hook(panic_info);
    }));
}

type Id = i64;

/// Chat state
struct View {
    dialog: Dialog,
    messages: Vec<Message>,
}

impl View {
    fn new(dialog: Dialog) -> View {
        View {
            dialog,
            messages: Vec::new(),
        }
    }
}

struct App {
    quit: bool,
    // dialogs: Vec<Dialog>,
    // messages: Vec<Message>,
    views: Vec<View>,
    active_view: Option<usize>,
}

impl App {
    fn new() -> Self {
        App {
            quit: false,
            // dialogs: Vec::new(),
            // messages: Vec::new(),
            views: Vec::new(),
            active_view: None,
        }
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.size();

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Min(0), Constraint::Max(3)])
        .split(area);

    let view_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![Constraint::Percentage(33), Constraint::Percentage(67)])
        .split(layout[0]);

    let dialogs_widget = List::new(
        app.views
            .iter()
            .map(|c| {
                format!(
                    "[{}]: {}",
                    c.dialog.chat().name(),
                    c.dialog.last_message.as_ref().map(|m| m.text()).unwrap_or("")
                )
            })
            .map(Line::from),
    )
    .block(Block::default().borders(Borders::ALL));

    let active_chat_widget = List::new(
        app.views[0].messages
            .iter()
            .map(|m| {
                m.text()
            })
            .map(Line::from),
    )
        .direction(ListDirection::BottomToTop)
        .block(Block::default().borders(Borders::ALL));

    let status_view =
        Paragraph::new(Line::from("tg9 v0.1")).block(Block::new().borders(Borders::ALL));

    frame.render_widget(dialogs_widget, view_layout[0]);
    frame.render_widget(active_chat_widget, view_layout[1]);
    frame.render_widget(status_view, layout[1]);
}

/// Jobs for api client worker to perform
#[derive(Debug)]
enum ApiJob {
    /// Load a part of chat messages
    LoadMessages,

    /// Initial loading of all dialogs
    LoadDialogs,
}

/// Perform API calls requested by application actions
async fn api_worker(client: Client, mut rx: UnboundedReceiver<ApiJob>, tx: UnboundedSender<ApiEvent>) {
    loop {
        let job = rx.recv().await.unwrap();
        tokio::spawn(async move {
            match job {
                ApiJob::LoadDialogs => {
                    let mut dialogs = client.iter_dialogs();
                    while let Some(dialog) = dialogs.next().await.unwrap() {
                        tx.send(ApiEvent::LoadMessages(()))
                    }
                }
                ApiJob::LoadMessages => {
                    // let mut messages = client.iter_messages(..).limit(40);
                    //
                    // while let Some(message) = messages.next().await.unwrap() {
                    //      tx.send(..)
                    // }
                }
            }
        });
    }
}

async fn receive_api_updates(client: Client, tx: UnboundedSender<ApiEvent>) {
    while let Some(update) = client.next_update().await.unwrap() {
        match update {
            Update::NewMessage(message) if !message.outgoing() => {
                tx.send(ApiEvent::Message(message)).unwrap();
            }
            Update::MessageDeleted(_message_del) => {
            }
            Update::MessageEdited(_message) => {}
            _ => {}
        }
    }
}

/// Events that update state from API messages
#[derive(Debug)]
enum ApiEvent {
    Message(Message),
    DeleteMessage(MessageDeletion),
    EditMessage(Message),
    LoadMessages(_),
    LoadDialogs(_),
    Error(_),
}

async fn run() -> Result<()> {
    // TODO: provide login data from tui
    let client = api::login().await?;

    let (mut api_tx, mut api_rx) = mpsc::unbounded_channel();

    let (mut api_job_tx, mut api_job_rx) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        api_worker(client.clone(), api_job_rx, api_tx.clone()).await;
    });

    let (mut screen_tx, mut screen_rx) = mpsc::unbounded_channel();
    let mut screen = screen::Screen::new(screen_tx.clone()).unwrap();
    screen.enter()?;

    let mut app = App::new();

    tokio::spawn(async move {
        receive_api_updates(client.clone(), api_tx.clone()).await;
    });

    api_job_tx.send(ApiJob::LoadDialogs);

    loop {
        tokio::select! {
            Some(e) = screen_rx.recv() => { 
                match e {
                    ScreenEvent::Quit => {},
                    ScreenEvent::Tick => {},
                    ScreenEvent::Render => {},

                    ScreenEvent::Key(e) => {
                        match (e.modifiers, e.code) {
                            (KeyModifiers::NONE, KeyCode::Char('q')) | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                                app.quit = true;
                            }
                            (KeyModifiers::NONE, KeyCode::Char('j')) => {
                                // let action_tx = app.action_tx.clone();
                                // app.active_view = Some(app.active_view.unwrap_or(0)+1);
                                // let c = &app.views[app.active_view.unwrap()].dialog.chat().clone();
                                // tokio::spawn(async move {
                                //     let mut messages = client.iter_messages(c).limit(40);
                                //
                                //     while let Some(message) = messages.next().await.unwrap() {
                                //         action_tx.send(Action::Message(message));
                                //     }
                                // });
                            }
                            _ => {}
                        }
                    },
                    ScreenEvent::Quit => app.quit = true,
                    _ => {}
                }
            }

            Some(api_event) = api_rx.recv() => {
                match api_event {
                    ApiEvent::Message(_message) => {
                    }
                    ApiEvent::LoadMessages(_) => {
                    }
                    ApiEvent::Error(_) => {
                    }
                }
            }
        }

        screen.terminal.draw(|f| {
            ui(f, &mut app);
        })?;

        if app.quit {
            break;
        }
    }

    screen.exit()?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_panic_handler();

    let result = run().await;

    result?;

    Ok(())
}
