mod api;
mod tui;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::KeyCode::Char;
use grammers_client::types::{Dialog, Message};
use grammers_client::{Client, Update};
use ratatui::{prelude::*, widgets::*};
use std::sync::Arc;
use tokio::sync::mpsc::{self, UnboundedSender};
use tui::Event;

pub fn setup_panic_handler() {
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        crossterm::execute!(std::io::stderr(), crossterm::terminal::LeaveAlternateScreen).unwrap();
        crossterm::terminal::disable_raw_mode().unwrap();
        original_hook(panic_info);
    }));
}

type Id = i64;

// struct Dialog {
//     id: Id,
//     name: String,
//     // last message, messages, etc
// }
//
struct App {
    should_quit: bool,
    action_tx: UnboundedSender<Action>,
    counter: i64,
    dialogs: Vec<Dialog>,
    messages: Vec<Message>,
    active_chat_id: Option<Id>,
}

impl App {
    fn new(action_tx: UnboundedSender<Action>) -> Self {
        App {
            should_quit: false,
            action_tx,
            counter: 0,
            dialogs: Vec::new(),
            messages: Vec::new(),
            active_chat_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Quit,
    Render,
    None,
    Tick,
    Down,
    Up,
    Dialog(Dialog),
    Message(Message),
}

impl Action {
    fn from_event(_app: &App, event: Event) -> Action {
        match event {
            Event::Error => Action::None,
            Event::Tick => Action::Tick,
            Event::Render => Action::Render,
            Event::Key(key) => match key.code {
                Char('j') => Action::Down,
                Char('k') => Action::Up,
                Char('q') => Action::Quit,
                _ => Action::None,
            },
            _ => Action::None,
        }
    }
}

fn ui(frame: &mut Frame, app: &mut App) {
    let area = frame.size();

    let items = app
        .dialogs
        .iter()
        .map(|d| {
            format!(
                "{} {}",
                d.chat().name(),
                d.last_message.as_ref().map(|m| m.text()).unwrap_or("")
            )
        })
        .map(ListItem::new);
    let list = List::new(items)
        .block(Block::default().title("List").borders(Borders::ALL))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().add_modifier(Modifier::ITALIC))
        .highlight_symbol(">>")
        .repeat_highlight_symbol(true)
        .direction(ListDirection::BottomToTop);

    frame.render_widget(list, area);
}

fn update(app: &mut App, action: Action) {
    match action {
        // Action::NetworkRequestAndThenIncrement => {
        //     let tx = app.action_tx.clone();
        //     tokio::spawn(async move {
        //         tokio::time::sleep(Duration::from_secs(5)).await; // simulate network request
        //         tx.send(Action::Increment).unwrap();
        //     });
        // }
        // Action::NetworkRequestAndThenDecrement => {
        //     let tx = app.action_tx.clone();
        //     tokio::spawn(async move {
        //         tokio::time::sleep(Duration::from_secs(5)).await; // simulate network request
        //         tx.send(Action::Decrement).unwrap();
        //     });
        // }
        Action::Quit => app.should_quit = true,
        Action::Dialog(dialog) => {
            // this should probably be a hashmap, etc
            app.dialogs.push(dialog);
        }
        Action::Message(message) => {}
        Action::None => {}
        Action::Tick => {}
        Action::Render => {}
        Action::Up => {}
        Action::Down => {}
    };
}

fn init_state(app: &App, client: Client) {
    let action_tx = app.action_tx.clone();
    tokio::spawn(async move {
        let mut dialogs = client.iter_dialogs();
        while let Some(dialog) = dialogs.next().await.unwrap() {
            action_tx.send(Action::Dialog(dialog)).unwrap();
        }
    });
}

fn listen_updates(app: &App, client: Client) {
    let action_tx = app.action_tx.clone();
    tokio::spawn(async move {
        while let Some(update) = client.next_update().await.unwrap() {
            match update {
                Update::NewMessage(mut message) if !message.outgoing() => {
                    // TODO: maybe use these types directly for simplicity
                    action_tx.send(Action::Message(message)).unwrap();
                    // action_tx.send(message)
                    // message.respond(message.text()).await?;
                }
                Update::MessageDeleted(del) => {}
                Update::MessageEdited(message) => {}
                _ => {}
            }
        }
    });
}

async fn run() -> Result<()> {
    let (action_tx, mut action_rx) = mpsc::unbounded_channel();

    let mut client = api::login().await?;

    let mut tui = tui::Tui::new()?
        .tick_rate(1.0)
        .frame_rate(30.0)
        .mouse(true)
        .paste(true);
    tui.enter()?;

    let mut app = App::new(action_tx.clone());

    init_state(&app, client.clone());
    listen_updates(&app, client.clone());

    loop {
        if let Some(e) = tui.next().await {
            match e {
                tui::Event::Quit => action_tx.send(Action::Quit)?,
                tui::Event::Tick => action_tx.send(Action::Tick)?,
                tui::Event::Render => action_tx.send(Action::Render)?,
                tui::Event::Key(_) => action_tx.send(Action::from_event(&app, e))?,
                _ => {}
            }
        };

        while let Ok(action) = action_rx.try_recv() {
            update(&mut app, action.clone());

            if let Action::Render = action {
                tui.draw(|f| {
                    ui(f, &mut app);
                })?;
            }
        }

        if app.should_quit {
            break;
        }
    }

    tui.exit()?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_panic_handler();

    let result = run().await;

    result?;

    Ok(())
}
