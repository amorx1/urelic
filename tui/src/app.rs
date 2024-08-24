use crate::{
    backend::{Backend as AppBackend, Bounds, UIEvent},
    dataset::{Dataset, Datasets},
    input::Inputs,
    query::{NRQLQuery, NRQL},
    ui::{
        render_dashboard, render_graph, render_load_session, render_loading, render_query_box,
        render_query_list, render_rename_dialog, render_save_session, render_splash,
    },
};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use rand::{thread_rng, Rng};
use ratatui::{
    backend::Backend,
    layout::{Constraint, Layout},
    style::{palette::tailwind::Palette, Color},
    widgets::ListState,
    Frame, Terminal,
};
use std::{
    collections::{btree_map::Entry, BTreeMap},
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    time::Duration,
};
use tokio::io;

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    QueryInput = 0,
    Rename = 1,
    Dashboard = 4,
    SessionLoad = 2,
    SessionSave = 5,
    Default = 3,
}

pub enum InputMode {
    Normal,
    Input,
}

pub struct Theme {
    pub focus_fg: Color,
    pub chart_fg: Color,
}

pub struct Session {
    pub is_loaded: bool,
    pub queries: Option<BTreeMap<String, String>>,
    pub session_path: Box<PathBuf>,
}

pub struct App {
    pub session: Session,
    pub theme: Theme,
    pub inputs: Inputs,
    pub input_mode: InputMode,
    pub focus: Focus,
    pub backend: AppBackend,
    pub list_state: ListState,
    pub datasets: Datasets,
    pub facet_colours: BTreeMap<String, Color>,
}

impl App {
    pub fn new(palette: &Palette, backend: AppBackend, session: Session) -> Self {
        Self {
            inputs: Inputs::new(),
            session,
            theme: Theme {
                focus_fg: palette.c500,
                chart_fg: palette.c900,
            },
            input_mode: InputMode::Normal,
            focus: Focus::Default,
            backend,
            list_state: ListState::default(),
            datasets: Datasets::new(),
            facet_colours: BTreeMap::default(),
        }
    }

    pub fn run<B: Backend>(mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        let mut rng = thread_rng();
        loop {
            terminal.draw(|f| self.ui(f))?;

            // Session Load
            if !self.session.is_loaded {
                self.focus = Focus::SessionLoad;
                self.set_input_mode(InputMode::Input);
            }

            // Event handlers
            if let Ok(true) = event::poll(Duration::from_millis(50)) {
                if let Event::Key(key) = event::read()? {
                    match self.input_mode {
                        InputMode::Normal if key.kind == KeyEventKind::Press => match key.code {
                            KeyCode::Char('q') => {
                                self.set_focus(Focus::SessionSave);
                                self.set_input_mode(InputMode::Input);
                            }
                            KeyCode::Char('e') => {
                                self.set_focus(Focus::QueryInput);
                                self.set_input_mode(InputMode::Input);
                            }
                            KeyCode::Char('j') => self.next(),
                            KeyCode::Char('k') => self.previous(),
                            KeyCode::Char('x') => self.delete_query(),
                            KeyCode::Char('r') => match self.focus {
                                Focus::QueryInput => {}
                                _ => {
                                    if !self.datasets.is_empty() {
                                        self.set_focus(Focus::Rename);
                                        self.set_input_mode(InputMode::Input);
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
                                        if let Ok(query) =
                                            self.inputs.get(Focus::QueryInput).to_nrql()
                                        {
                                            self.add_query(query);
                                        }
                                    }
                                    Focus::Rename => {
                                        self.rename_query(
                                            self.datasets.selected.to_owned(),
                                            self.inputs.get(Focus::Rename).to_owned(),
                                        );
                                    }
                                    Focus::SessionLoad => {
                                        match self.inputs.get(Focus::SessionLoad) {
                                            // Load session
                                            "y" | "Y" => {
                                                self.load_session();
                                            }
                                            // Don't load session
                                            _ => {}
                                        }
                                        // Update focus to home
                                        self.set_focus(Focus::Default);
                                    }
                                    Focus::SessionSave => {
                                        match self.inputs.get(Focus::SessionSave) {
                                            // Load session
                                            "y" | "Y" => {
                                                self.save_session();
                                            }
                                            _ => {}
                                        }
                                        return Ok(());
                                    }
                                    _ => {}
                                };
                                self.inputs.clear(self.focus);
                                self.inputs.reset_cursor(self.focus);
                                self.set_focus(Focus::Default);
                                self.set_input_mode(InputMode::Normal);
                            }
                            KeyCode::Char(to_insert) => {
                                self.inputs.enter_char(self.focus, to_insert);
                            }
                            KeyCode::Backspace => {
                                self.inputs.delete_char(self.focus);
                            }
                            KeyCode::Left => {
                                self.inputs.move_cursor_left(self.focus);
                            }
                            KeyCode::Right => {
                                self.inputs.move_cursor_right(self.focus);
                            }
                            KeyCode::Esc => {
                                self.set_focus(Focus::Default);
                                self.set_input_mode(InputMode::Normal);
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
                        has_data: true,
                    });
                } else {
                    _ = self
                        .datasets
                        .entry(payload.query.to_owned())
                        .and_modify(|data| {
                            data.facets = payload.data;
                            data.bounds = payload.bounds;
                            data.has_data = true
                        })
                }

                for facet_key in payload.facets {
                    // Only add facet key if not present
                    if let Entry::Vacant(e) = self.facet_colours.entry(facet_key) {
                        e.insert(Color::Rgb(
                            rng.gen::<u8>(),
                            rng.gen::<u8>(),
                            rng.gen::<u8>(),
                        ));
                    }
                }
            }
        }
    }

    pub fn ui(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let horizontal = Layout::horizontal([Constraint::Percentage(15), Constraint::Min(20)]);
        let vertical = Layout::vertical([Constraint::Length(3), Constraint::Min(20)]);
        let [input_area, rest] = vertical.areas(area);
        let [list_area, graph_area] = horizontal.areas(rest);

        match self.focus {
            Focus::SessionSave => render_save_session(self, frame, area),
            Focus::SessionLoad => render_load_session(self, frame, area),
            Focus::Dashboard => render_dashboard(self, frame, area),
            Focus::Rename => {
                render_query_box(self, frame, input_area);
                render_query_list(self, frame, list_area);
                render_rename_dialog(self, frame, graph_area);
            }
            Focus::Default | Focus::QueryInput => {
                render_query_box(self, frame, input_area);
                render_query_list(self, frame, list_area);
                if let Some(dataset) = self.datasets.selected() {
                    if dataset.has_data {
                        render_graph(self, frame, graph_area);
                    } else {
                        render_loading(self, frame, graph_area);
                    }
                } else {
                    render_splash(self, frame, graph_area);
                }
            }
        }
    }

    fn rename_query(&mut self, query: String, alias: String) {
        if let Entry::Vacant(e) = self.datasets.entry(query.to_owned()) {
            e.insert(Dataset {
                has_data: false,
                query_alias: Some(alias),
                facets: BTreeMap::default(),
                bounds: Bounds::default(),
                selection: String::new(),
            });
        } else {
            _ = self.datasets.entry(query.to_owned()).and_modify(|data| {
                data.query_alias = Some(alias);
            })
        }
    }

    fn add_query(&self, query: NRQLQuery) {
        self.backend.add_query(query);
    }

    pub fn set_focus(&mut self, focus: Focus) {
        self.focus = focus
    }

    pub fn set_input_mode(&mut self, mode: InputMode) {
        self.input_mode = mode;
    }

    pub fn delete_query(&mut self) {
        let i = self.list_state.selected().unwrap();

        let removed = self.datasets.remove_entry(i);
        // TODO: Fix deleted queries reappearing on new data!
        _ = self.backend.ui_tx.send(UIEvent::DeleteQuery(removed));
    }

    pub fn next(&mut self) {
        if self.datasets.is_empty() {
            return;
        }

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
        self.datasets.select(i);
    }

    pub fn previous(&mut self) {
        if self.datasets.is_empty() {
            return;
        }

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
        self.datasets.select(i);
    }

    pub fn load_session(&mut self) {
        let session_path = self.session.session_path.clone();
        let yaml = fs::read_to_string(*session_path).expect("ERROR: Could not read session file!");
        let session_queries: Option<BTreeMap<String, String>> =
            serde_yaml::from_str(&yaml).expect("ERROR: Could not deserialize session file!");

        if let Some(queries) = session_queries {
            let iter = queries.into_iter();
            for (alias, query) in iter {
                let clean_query = query.replace("as value", "");
                if let Ok(parsed_query) = clean_query.trim().to_nrql() {
                    self.add_query(parsed_query.clone());
                    self.rename_query(parsed_query.to_string().unwrap(), alias);
                }
            }
        }

        self.session.is_loaded = true;
    }

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
        let session_path = self.session.session_path.clone();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .create(true)
            .open(*session_path)
            .expect("ERROR: Could not open file!");
        file.write_all(yaml.as_bytes())
            .expect("ERROR: Could not write to file!");
    }
}
