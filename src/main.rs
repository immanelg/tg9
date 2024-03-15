mod screen;
mod api;

use anyhow::Result;
use crossterm::event::KeyCode::Char;
use grammers_client::types::iter_buffer::InvocationError;
use grammers_client::types::{Dialog, Message, Chat};
use grammers_client::{Client, Update};
use ratatui::{prelude::*, widgets::*};

use tokio::sync::mpsc::{self, UnboundedSender};
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

// fn update(app: &mut App, action: Action, client: Client) {
//     match action {
//         Action::Quit => app.quit = true,
//         Action::Dialog(dialog) => {
//             // app.dialogs.push(dialog);
//             app.views.push(View::new(dialog))
//         }
//         Action::Message(_message) => {}
//         Action::None => {}
//         Action::Tick => {}
//         Action::Render => {}
//         Action::DialogUp => {
//         }
//         Action::DialogDown => {
//             let action_tx = app.action_tx.clone();
//             app.active_view = Some(app.active_view.unwrap_or(0)+1);
//             let c = &app.views[app.active_view.unwrap()].dialog.chat().clone();
//             tokio::spawn(async move {
//                 let mut messages = client.iter_messages(c).limit(40);
//
//                 while let Some(message) = messages.next().await.unwrap() {
//                     action_tx.send(Action::Message(message));
//                 }
//             });
//         }
//     };
// }

fn init_dialogs(app: &App, client: Client) {
    let action_tx = app.action_tx.clone();
    tokio::spawn(async move {
        let mut dialogs = client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await.unwrap() {
            action_tx.send(Action::Dialog(dialog)).unwrap();
        }
    });
}

fn receive_updates(app: &App, client: Client) {
    let action_tx = app.action_tx.clone();
    tokio::spawn(async move {
        while let Some(update) = client.next_update().await.unwrap() {
            match update {
                Update::NewMessage(message) if !message.outgoing() => {
                    action_tx.send(Action::Message(message)).unwrap();
                    // message.respond(message.text()).await?;
                }
                Update::MessageDeleted(_del) => {}
                Update::MessageEdited(_message) => {}
                _ => {}
            }
        }
    });
}

#[derive(Debug)]
enum ApiEvent {
    Message(Message),
    LoadMessages(Dialog),
    Error(InvocationError),
}

async fn run() -> Result<()> {
    // let (action_tx, mut action_rx) = mpsc::unbounded_channel();

    let client = api::login().await?;

    let (screen_tx, screen_rx) = mpsc::unbounded_channel();
    let mut screen = screen::Screen::new(screen_tx.clone());
    screen.enter()?;

    let mut app = App::new();

    init_dialogs(&app, client.clone());
    receive_updates(&app, client.clone());

    loop {
        tokio::select! {
            Some(e) = screen.next() => { 
                match e {
                    ScreenEvent::Quit => {},
                    ScreenEvent::Tick => {},
                    ScreenEvent::Render => {},
                    ScreenEvent::Key(e) => {
                        match (e.modifiers, e.code) {
                            _ => {}
                        }
                    },
                    ScreenEvent::Quit => app.quit = true,
                    _ => {}
                }
            }

            Some(api_event) = next_api_event() => {
                match api_event {
                    // ApiEvent::Dialog(dialog) => {
                    //     // app.dialogs.push(dialog);
                    //     app.views.push(View::new(dialog))
                    // }
                    ApiEvent::Message(_message) => {
                    }
                    ApiEvent::LoadMessages(_) => {
                    }
                    ApiEvent::Error(_) => {
                    }
                    Action::DialogDown => {
                        let action_tx = app.action_tx.clone();
                        app.active_view = Some(app.active_view.unwrap_or(0)+1);
                        let c = &app.views[app.active_view.unwrap()].dialog.chat().clone();
                        tokio::spawn(async move {
                            let mut messages = client.iter_messages(c).limit(40);

                            while let Some(message) = messages.next().await.unwrap() {
                                action_tx.send(Action::Message(message));
                            }
                        });
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
