use crate::{
    backend::{Backend as AppBackend, Bounds},
    query::{NRQLQuery, NRQL},
    ui::{
        render_dashboard, render_graph, render_load_session, render_loading, render_query_box,
        render_query_list, render_rename_dialog,
    },
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Layout},
    style::{
        palette::tailwind::{self, Palette},
        Color,
    },
    widgets::ListState,
    Frame, Terminal,
};
use std::{
    collections::{btree_map::Entry, BTreeMap},
    fs::File,
    io::Write,
    time::Duration,
};
use tokio::io;

pub const QUERY: isize = 0;
pub const RENAME: isize = 1;
pub const SESSION_LOAD: isize = 2;
pub const DEFAULT: isize = 3;
pub const DASHBOARD: isize = 4;
pub const LOADING: isize = 5;

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    QueryInput = QUERY,
    Rename = RENAME,
    Dashboard = DASHBOARD,
    SessionLoad = SESSION_LOAD,
    Loading = LOADING,
    Default = DEFAULT,
}

pub enum InputMode {
    Normal,
    Input,
}

pub struct Input {
    pub buffer: String,
    pub cursor_position: usize,
}

pub struct Dataset {
    pub query_alias: Option<String>,
    pub facets: BTreeMap<String, Vec<(f64, f64)>>,
    pub bounds: Bounds,
    pub selection: String,
}

pub struct Theme {
    pub focus_fg: Color,
    pub chart_fg: Color,
    pub elastic_fg: Color,
    pub net_fg: Color,
    pub webex_fg: Color,
    pub value_fg: Color,
}

pub struct App {
    pub session: Option<BTreeMap<String, String>>,
    pub theme: Theme,
    pub inputs: [Input; 4],
    pub input_mode: InputMode,
    pub focus: Focus,
    pub backend: AppBackend,
    pub selected_query: String,
    pub list_state: ListState,
    pub datasets: BTreeMap<String, Dataset>,
}

impl App {
    pub fn new(
        palette: &Palette,
        backend: AppBackend,
        session: Option<BTreeMap<String, String>>,
    ) -> Self {
        Self {
            inputs: [
                Input {
                    buffer: "".to_owned(),
                    cursor_position: 0,
                },
                Input {
                    buffer: "".to_owned(),
                    cursor_position: 0,
                },
                Input {
                    buffer: "".to_owned(),
                    cursor_position: 0,
                },
                Input {
                    buffer: "".to_owned(),
                    cursor_position: 0,
                },
            ],
            session,
            theme: Theme {
                focus_fg: palette.c500,
                chart_fg: palette.c900,
                elastic_fg: palette.c400,
                net_fg: palette.c400,
                webex_fg: tailwind::AMBER.c400,
                value_fg: palette.c400,
            },
            input_mode: InputMode::Normal,
            focus: Focus::Default,
            backend,
            selected_query: String::new(),
            list_state: ListState::default(),
            datasets: BTreeMap::default(),
        }
    }

    pub fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            if self.session.is_some() {
                self.focus = Focus::SessionLoad;
                self.input_mode = InputMode::Input;
            }

            // Manual event handlers.
            if let Ok(true) = event::poll(Duration::from_millis(50)) {
                if let Event::Key(key) = event::read()? {
                    match self.input_mode {
                        InputMode::Normal if key.kind == KeyEventKind::Press => match key.code {
                            KeyCode::Char('q') => return Ok(()),
                            KeyCode::Char('e') => {
                                self.set_focus(Focus::QueryInput);
                                self.input_mode = InputMode::Input;
                            }
                            KeyCode::Char('j') => self.next(),
                            KeyCode::Char('k') => self.previous(),
                            KeyCode::Char('x') => self.delete(),
                            KeyCode::Char('r') => match self.focus {
                                Focus::QueryInput => {}
                                _ => {
                                    if self.datasets.len() != 0 {
                                        self.set_focus(Focus::Rename);
                                        self.input_mode = InputMode::Input;
                                    }
                                }
                            },
                            KeyCode::Char('d') => match self.focus {
                                Focus::Dashboard => self.set_focus(Focus::Default),
                                _ => self.set_focus(Focus::Dashboard),
                            },
                            _ => (),
                        },
                        InputMode::Input if key.kind == KeyEventKind::Press => match key.code {
                            KeyCode::Enter => {
                                match self.focus {
                                    Focus::QueryInput => {
                                        if let Ok(query) = self.input_buffer(QUERY).to_nrql() {
                                            self.add_query(query);
                                        }
                                    }
                                    Focus::Rename => {
                                        self.rename_current_query();
                                    }
                                    Focus::SessionLoad => {
                                        match self.input_buffer(SESSION_LOAD) {
                                            // Load session
                                            "y" | "Y" => {
                                                let mut session =
                                                    self.session.clone().unwrap().into_iter();
                                                while let Some((_alias, query)) = session.next() {
                                                    if let Ok(query) = query.trim().to_nrql() {
                                                        self.add_query(query);
                                                        // self.set_focus(Focus::Loading);
                                                    }
                                                }
                                                // };
                                            }
                                            // Don't load session
                                            _ => {}
                                        }
                                        // Clear previous session once loaded
                                        self.session = None;

                                        // Update focus to home
                                        self.set_focus(Focus::Default);
                                    }
                                    _ => {}
                                };
                                self.inputs[self.focus as usize].buffer.clear();
                                self.reset_cursor();
                                self.set_focus(Focus::Default);
                                self.input_mode = InputMode::Normal;
                            }
                            KeyCode::Char(to_insert) => {
                                self.enter_char(to_insert);
                            }
                            KeyCode::Backspace => {
                                self.delete_char();
                            }
                            KeyCode::Left => {
                                self.move_cursor_left();
                            }
                            KeyCode::Right => {
                                self.move_cursor_right();
                            }
                            KeyCode::Esc => {
                                self.set_focus(Focus::Default);
                                self.input_mode = InputMode::Normal;
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }

            while let Some(payload) = self.backend.data_rx.try_iter().next() {
                if let Entry::Vacant(e) = self.datasets.entry(payload.query.clone()) {
                    e.insert(Dataset {
                        query_alias: None,
                        facets: payload.data,
                        bounds: payload.bounds,
                        selection: payload.selection,
                    });
                } else {
                    _ = self
                        .datasets
                        .entry(payload.query.to_owned())
                        .and_modify(|data| {
                            data.facets = payload.data;
                            data.bounds = payload.bounds;
                        })
                }
            }
        }
    }

    pub fn ui(&mut self, frame: &mut Frame) {
        if self.focus == Focus::Loading {
            render_loading(self, frame, frame.size());
        }
        if self.focus == Focus::SessionLoad {
            render_load_session(self, frame, frame.size());
            return;
        }
        if self.focus == Focus::Dashboard {
            render_dashboard(self, frame, frame.size());
            return;
        }
        let area = frame.size();
        // TODO: Possible to pre-compute?
        let horizontal = Layout::horizontal([Constraint::Percentage(15), Constraint::Min(20)]);
        let vertical = Layout::vertical([Constraint::Length(3), Constraint::Min(20)]);
        let [input_area, rest] = vertical.areas(area);
        let [list_area, graph_area] = horizontal.areas(rest);

        render_query_box(self, frame, input_area);
        render_query_list(self, frame, list_area);
        match self.focus {
            Focus::Default | Focus::QueryInput => {
                render_graph(self, frame, graph_area);
            }
            Focus::Rename => {
                render_rename_dialog(self, frame, graph_area);
            }
            // Should never be reached
            _ => panic!(),
        }
    }

    fn rename_current_query(&mut self) {
        self.datasets
            .entry(self.selected_query.to_owned())
            .and_modify(|v| v.query_alias = Some(self.inputs[RENAME as usize].buffer.to_owned()));
    }

    pub fn input_buffer(&self, focus: isize) -> &str {
        self.inputs[focus as usize].buffer.as_str()
    }

    fn add_query(&self, query: NRQLQuery) {
        self.backend.add_query(query);
    }

    fn clamp_cursor(&self, new_cursor_pos: usize) -> usize {
        new_cursor_pos.clamp(0, self.inputs[self.focus as usize].buffer.len())
    }

    fn reset_cursor(&mut self) {
        self.inputs[self.focus as usize].cursor_position = 0;
    }

    fn move_cursor_left(&mut self) {
        let cursor_moved_left = self.inputs[self.focus as usize]
            .cursor_position
            .saturating_sub(1);
        self.inputs[self.focus as usize].cursor_position = self.clamp_cursor(cursor_moved_left);
    }

    fn move_cursor_right(&mut self) {
        let cursor_moved_right = self.inputs[self.focus as usize]
            .cursor_position
            .saturating_add(1);
        self.inputs[self.focus as usize].cursor_position = self.clamp_cursor(cursor_moved_right);
    }

    fn enter_char(&mut self, new_char: char) {
        let cursor_position = self.inputs[self.focus as usize].cursor_position;
        self.inputs[self.focus as usize]
            .buffer
            .insert(cursor_position, new_char);

        self.move_cursor_right();
    }

    fn delete_char(&mut self) {
        let is_not_cursor_leftmost = self.inputs[self.focus as usize].cursor_position != 0;
        if is_not_cursor_leftmost {
            let current_index = self.inputs[self.focus as usize].cursor_position;
            let from_left_to_current_index = current_index - 1;

            let before_char_to_delete = self.inputs[self.focus as usize]
                .buffer
                .chars()
                .take(from_left_to_current_index);
            let after_char_to_delete = self.inputs[self.focus as usize]
                .buffer
                .chars()
                .skip(current_index);

            self.inputs[self.focus as usize].buffer =
                before_char_to_delete.chain(after_char_to_delete).collect();
            self.move_cursor_left();
        }
    }

    pub fn set_focus(&mut self, focus: Focus) {
        self.focus = focus
    }

    pub fn delete(&mut self) {
        let i = self.list_state.selected().unwrap();
        let to_delete = self
            .datasets
            .keys()
            .nth(i)
            .cloned()
            .expect("ERROR: Could not index query for deletion!");

        let (removed, _) = self.datasets.remove_entry(&to_delete).unwrap();
        // TODO: Fix deleted queries reappearing on new data!
        _ = self.backend.ui_tx.send(removed);
    }

    pub fn next(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.datasets.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.selected_query = self
            .datasets
            .keys()
            .nth(i)
            .expect("ERROR: Could not select query!")
            .to_owned();
    }

    pub fn previous(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.datasets.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.selected_query = self
            .datasets
            .keys()
            .nth(i)
            .expect("ERROR: Could not select query!")
            .to_owned();
    }

    // TODO: Prompt user for session save on exit (q)
    pub fn save_session(&self) {
        let output = self
            .datasets
            .iter()
            .map(|(q, data)| {
                (
                    data.query_alias.clone().unwrap_or(q.to_owned()),
                    q.to_owned(),
                )
            })
            .collect::<BTreeMap<String, String>>();

        let yaml: String =
            serde_yaml::to_string(&output).expect("ERROR: Could not serialize queries!");
        let mut file = File::open("").expect("ERROR: Could not open file!");
        file.write_all(yaml.as_bytes())
            .expect("ERROR: Could not write to file!");
    }
}
