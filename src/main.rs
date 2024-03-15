mod api;
mod tui;

use anyhow::Result;
use crossterm::event::KeyCode::Char;
use grammers_client::types::{Dialog, Message};
use grammers_client::{Client, Update};
use ratatui::{prelude::*, widgets::*};

use tokio::sync::mpsc::{self, UnboundedSender};
use tui::TermEvent;

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
    action_tx: UnboundedSender<Action>,
    // dialogs: Vec<Dialog>,
    // messages: Vec<Message>,
    views: Vec<View>,
    active_view: Option<usize>,
}

impl App {
    fn new(action_tx: UnboundedSender<Action>) -> Self {
        App {
            quit: false,
            action_tx,
            // dialogs: Vec::new(),
            // messages: Vec::new(),
            views: Vec::new(),
            active_view: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Quit,
    Render,
    None,
    Tick,
    DialogDown,
    DialogUp,
    Dialog(Dialog),
    Message(Message),
}

impl Action {
    fn from_term_event(_app: &App, event: TermEvent) -> Action {
        match event {
            TermEvent::Error => Action::None,
            TermEvent::Tick => Action::Tick,
            TermEvent::Render => Action::Render,
            TermEvent::Key(key) => match key.code {
                Char('J') => Action::DialogDown,
                Char('K') => Action::DialogUp,
                Char('q') => Action::Quit,
                _ => Action::None,
            },
            _ => Action::None,
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

fn update(app: &mut App, action: Action, client: Client) {
    match action {
        Action::Quit => app.quit = true,
        Action::Dialog(dialog) => {
            // app.dialogs.push(dialog);
            app.views.push(View::new(dialog))
        }
        Action::Message(_message) => {}
        Action::None => {}
        Action::Tick => {}
        Action::Render => {}
        Action::DialogUp => {
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
    };
}

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

async fn run() -> Result<()> {
    let (action_tx, mut action_rx) = mpsc::unbounded_channel();

    let client = api::login().await?;

    let mut term = tui::TermApi::new()?;
    term.enter()?;

    let mut app = App::new(action_tx.clone());

    init_dialogs(&app, client.clone());
    receive_updates(&app, client.clone());

    loop {
        if let Some(e) = term.next().await {
            match e {
                TermEvent::Quit => action_tx.send(Action::Quit)?,
                TermEvent::Tick => action_tx.send(Action::Tick)?,
                TermEvent::Render => action_tx.send(Action::Render)?,
                TermEvent::Key(_) => action_tx.send(Action::from_term_event(&app, e))?,
                _ => {}
            }
        };

        while let Ok(action) = action_rx.try_recv() {
            update(&mut app, action.clone(), client.clone());

            if let Action::Render = action {
                term.terminal.draw(|f| {
                    ui(f, &mut app);
                })?;
            }
        }

        if app.quit {
            break;
        }
    }

    term.exit()?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    setup_panic_handler();

    let result = run().await;

    result?;

    Ok(())
}
