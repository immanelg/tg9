mod api;
mod screen;
mod ui;

use screen::ScreenEvent;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use grammers_client::types::iter_buffer::InvocationError;
use grammers_client::types::{Chat, Dialog, Message, MessageDeletion};
use grammers_client::{Client, Update};
use grammers_session::PackedChat;
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc;
use std::cmp;
use std::collections::{VecDeque, HashMap};


struct ChatState {
    chat: PackedChat,
    dialog: Dialog,
    messages: VecDeque<Message>,
}

impl ChatState {
    fn new(dialog: Dialog) -> ChatState {
        let chat = dialog.chat().pack();
        ChatState {
            dialog,
            messages: VecDeque::new(),
            chat,
        }
    }
}

struct App {
    quit: bool,
    chat_states: VecDeque<ChatState>,
    dialog_idx: Option<usize>,
    // chat_idxs: HashMap<usize, Option<usize>>,
}

impl App {
    fn new() -> Self {
        App {
            quit: false,
            chat_states: VecDeque::new(),
            dialog_idx: None,
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
        app.chat_states
            .iter()
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "{}[{}]: {}",
                    if app.dialog_idx == Some(i) { "*" } else { " " },
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

    let active_chat_widget = if let Some(idx) = app.dialog_idx {
        List::new(
            app.chat_states
                .get(idx)
                .unwrap()
                .messages
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
    LoadMessages(PackedChat),

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
            match job {
                ApiJob::LoadDialogs => {
                    let mut dialogs = client.iter_dialogs();
                    while let Some(dialog) = dialogs.next().await.unwrap() {
                        tx.send(ApiEvent::LoadedDialog(dialog)).unwrap();
                    }
                }
                ApiJob::LoadMessages(c) => {
                    // TODO: when scrolling up, load necessary messages. For now this is
                    // just for initial loading of chats (and the view is not scrollable)
                    let mut message_iter = client.iter_messages(c).limit(30);

                    // let mut messages = Vec::new();
                    while let Some(message) = message_iter.next().await.unwrap() {
                        // messages.push(message);
                        tx.send(ApiEvent::LoadedMessages(message)).unwrap();
                    }
                }
            }
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
                            if app.chat_states.is_empty() { continue; } 
                            let idx = cmp::min(app.dialog_idx.map(|i| i+1).unwrap_or(0), app.chat_states.len()-1);
                            app.dialog_idx = Some(idx);
                            let c = app.chat_states[idx].chat;
                            api_job_tx.send(ApiJob::LoadMessages(c)).unwrap();
                        }
                        (KeyModifiers::NONE, KeyCode::Char('k')) => {
                            if app.chat_states.is_empty() { continue; } 
                            let idx = cmp::max(app.dialog_idx.map(|i| i-1).unwrap_or(0), 0);
                            app.dialog_idx = Some(idx);
                            let chat = app.chat_states[idx].chat;
                            api_job_tx.send(ApiJob::LoadMessages(chat)).unwrap();
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
                        let chat_state = ChatState::new(dialog);
                        app.chat_states.push_back(chat_state);
                    }
                    ApiEvent::LoadedMessages(message) => {
                        for v in app.chat_states.iter_mut() {
                            if v.chat == message.chat().into() {
                                v.messages.push_back(message);
                                break;
                            }
                        }
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
    screen::setup_panic_handler();

    let result = run().await;

    result?;

    Ok(())
}
