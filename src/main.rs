mod api;
mod screen;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use grammers_client::types::iter_buffer::InvocationError;
use grammers_client::types::{Chat, Dialog, Message, MessageDeletion};
use grammers_client::{Client, Update};
use ratatui::{prelude::*, widgets::*};

use screen::ScreenEvent;
use tokio::sync::mpsc;

pub fn setup_panic_handler() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        crossterm::execute!(std::io::stderr(), crossterm::terminal::LeaveAlternateScreen).unwrap();
        crossterm::terminal::disable_raw_mode().unwrap();
        original_hook(panic_info);
    }));
}

/// Chat state
struct View {
    dialog: Dialog,
    messages_cache: Vec<Message>,
}

impl View {
    fn new(dialog: Dialog) -> View {
        View {
            dialog,
            messages_cache: Vec::new(),
        }
    }
}

struct App {
    quit: bool,
    views: Vec<View>,
    active_view: Option<usize>,
}

impl App {
    fn new() -> Self {
        App {
            quit: false,
            views: Vec::new(),
            active_view: None,
        }
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.size();

    // TODO: scroll and stuff
    //
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
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "{}[{}]: {}",
                    if app.active_view == Some(i) { "*" } else { " " },
                    c.dialog.chat().name(),
                    c.dialog
                        .last_message
                        .as_ref()
                        .map(|m| m.text())
                        .unwrap_or("")
                )
            })
            .map(Line::from),
    )
    .block(Block::default().borders(Borders::ALL));

    let active_chat_widget = if let Some(idx) = app.active_view {
        List::new(
            app.views
                .get(idx)
                .unwrap()
                .messages_cache
                .iter()
                .map(|m| m.text())
                .map(Line::from),
        )
    } else {
        List::default()
    }
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

/// Perform API calls and receive updates.
async fn api_worker(
    client: Client,
    mut rx: mpsc::UnboundedReceiver<ApiJob>,
    tx: mpsc::UnboundedSender<ApiEvent>,
) {
    loop {
        tokio::select! {
            job = rx.recv() => {
                let Some(job) = job else { break; };
                let tx = tx.clone();
                let client = client.clone();
                tokio::spawn(async move {
                    match job {
                        ApiJob::LoadDialogs => {
                            let mut dialogs = client.iter_dialogs();
                            while let Some(dialog) = dialogs.next().await.unwrap() {
                                tx.send(ApiEvent::LoadedDialog(dialog)).unwrap();
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
            update = client.next_update() => {
                let Ok(update) = update else {
                    tx.send(ApiEvent::Error()).unwrap();
                    break;
                };
                let Some(update) = update else { break; };
                match update {
                    Update::NewMessage(message) if !message.outgoing() => {
                        tx.send(ApiEvent::MessageNew(message)).unwrap();
                    }
                    Update::MessageDeleted(_message_del) => {}
                    Update::MessageEdited(_message) => {}
                    _ => {}
                }
            }
        }
    }
}

/// Events that update state from API messages
#[derive(Debug)]
enum ApiEvent {
    /// new message
    MessageNew(Message),

    MessageDeleted(MessageDeletion),

    MessageEdited(Message),

    /// load a part of messages in chat
    LoadedMessages(Message),

    /// initial loading of dialogs
    LoadedDialog(Dialog),

    /// error invoking API
    Error(),
}

async fn run() -> Result<()> {
    // TODO: provide login data from tui
    let client = api::login().await?;

    let (api_tx, mut api_rx) = mpsc::unbounded_channel();

    let (api_job_tx, api_job_rx) = mpsc::unbounded_channel();

    tokio::spawn({
        let api_tx = api_tx.clone();
        let client = client.clone();
        async move {
            api_worker(client, api_job_rx, api_tx).await;
        }
    });

    let (screen_tx, mut screen_rx) = mpsc::unbounded_channel();
    let mut screen = screen::Screen::new(screen_tx).unwrap();
    screen.enter()?;

    let mut app = App::new();

    api_job_tx.send(ApiJob::LoadDialogs).unwrap();

    loop {
        tokio::select! {
            Some(e) = screen_rx.recv() => {
                match e {
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
                    ApiEvent::LoadedDialog(dialog) => {
                        let view = View::new(dialog);
                        // api_job_tx.send(ApiJob::LoadMessages)
                        app.views.push(view);
                    }
                    ApiEvent::LoadedMessages(_message) => {

                    }
                    ApiEvent::MessageNew(_message) => {
                    }
                    ApiEvent::MessageDeleted(_deleted) => {
                    }
                    ApiEvent::MessageEdited(_message) => {
                    }
                    ApiEvent::Error() => {
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
