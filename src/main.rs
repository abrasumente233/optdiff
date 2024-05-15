use regex::Regex;
use std::collections::HashMap;

use std::{error::Error, io};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use itertools::Itertools;
use ratatui::{prelude::*, widgets::*};
use style::palette::tailwind;
use unicode_width::UnicodeWidthStr;

#[derive(Debug)]
struct Pass {
    name: String,
    machine: bool,
    after: String,
    before: String,
    ir_changed: bool,
}

type OptPipelineResults = HashMap<String, Vec<Pass>>;

#[allow(dead_code)]
#[derive(Debug)]
struct OptPipelineBackendOptions {
    filter_debug_info: bool,
    filter_ir_metadata: bool,
    full_module: bool,
    no_discard_value_names: bool,
    demangle: bool,
    library_functions: bool,
}

#[derive(Debug)]
struct PassDump {
    header: String,
    affected_function: Option<String>,
    machine: bool,
    lines: String,
}

#[derive(Debug)]
struct SplitPassDump {
    header: String,
    machine: bool,
    functions: HashMap<String, Vec<String>>,
}

struct LlvmPassDumpParser {
    filters: Vec<Regex>,
    line_filters: Vec<Regex>,
    debug_info_filters: Vec<Regex>,
    debug_info_line_filters: Vec<Regex>,
    metadata_line_filters: Vec<Regex>,
    ir_dump_header: Regex,
    machine_code_dump_header: Regex,
    function_define: Regex,
    machine_function_begin: Regex,
    function_end: Regex,
    machine_function_end: Regex,
}

impl LlvmPassDumpParser {
    fn new() -> Self {
        LlvmPassDumpParser {
            filters: vec![
                Regex::new(r"^; ModuleID = '.+'$").unwrap(),
                Regex::new(r"^(source_filename|target datalayout|target triple) = '.+'$").unwrap(),
                Regex::new(r"^; Function Attrs: .+$").unwrap(),
                Regex::new(r"^declare .+$").unwrap(),
                Regex::new(r"^attributes #\d+ = \{ .+ \}$").unwrap(),
            ],
            //line_filters: vec![Regex::new(r",? #\d+((?=( {)?$))").unwrap()],
            line_filters: vec![Regex::new(r",? #\d+( \{)?$").unwrap()],
            debug_info_filters: vec![
                Regex::new(r"^\s+(tail\s)?call void @llvm\.dbg.+$").unwrap(),
                Regex::new(r"^\s+DBG_.+$").unwrap(),
                Regex::new(r"^(!\d+) = (?:distinct )?!DI([A-Za-z]+)\(([^)]+?)\)").unwrap(),
                Regex::new(r"^(!\d+) = (?:distinct )?!\{.*\}").unwrap(),
                Regex::new(r"^(![.A-Z_a-z-]+) = (?:distinct )?!\{.*\}").unwrap(),
            ],
            debug_info_line_filters: vec![
                Regex::new(r",? !dbg !\d+").unwrap(),
                Regex::new(r",? debug-location !\d+").unwrap(),
            ],
            metadata_line_filters: vec![Regex::new(r",?(?: ![\d.A-Za-z]+){2}").unwrap()],
            ir_dump_header: Regex::new(
                r"^;?\s?\*{3} (.+) \*{3}(?:\s+\((?:function: |loop: )(%?[\w$.]+)\))?(?:;.+)?$",
            )
            .unwrap(),
            machine_code_dump_header: Regex::new(r"^# \*{3} (.+) \*{3}:$").unwrap(),
            function_define: Regex::new(r"^define .+ @([\w.]+|'[^']+')\(.+$").unwrap(),
            machine_function_begin: Regex::new(r"^# Machine code for function ([\w$.]+):.*$")
                .unwrap(),
            function_end: Regex::new(r"^}$").unwrap(),
            machine_function_end: Regex::new(r"^# End machine code for function ([\w$.]+).$")
                .unwrap(),
        }
    }

    fn breakdown_output_into_pass_dumps(&self, ir: String) -> Vec<PassDump> {
        let mut raw_passes = Vec::new();
        let mut pass: Option<PassDump> = None;
        let mut last_was_blank = false;

        for line in ir.lines() {
            if self.machine_code_dump_header.is_match(line) {
                break;
            }
            let ir_match = self.ir_dump_header.captures(line);
            let machine_match = self.machine_code_dump_header.captures(line);
            let machine_match_is_some = machine_match.is_some();
            let header = ir_match.or(machine_match);

            if let Some(header) = header {
                if let Some(current_pass) = pass.take() {
                    raw_passes.push(current_pass);
                }
                pass = Some(PassDump {
                    header: header.get(1).unwrap().as_str().to_string(),
                    affected_function: header.get(2).map(|m| m.as_str().to_string()),
                    machine: machine_match_is_some,
                    lines: String::new(),
                });
                last_was_blank = true;
            } else if let Some(ref mut current_pass) = pass {
                if line.trim().is_empty() {
                    if !last_was_blank {
                        current_pass.lines += line;
                        current_pass.lines += "\n";
                    }
                    last_was_blank = true;
                } else {
                    current_pass.lines += line;
                    current_pass.lines += "\n";
                    last_was_blank = false;
                }
            }
        }
        if let Some(current_pass) = pass {
            raw_passes.push(current_pass);
        }
        raw_passes
    }

    fn breakdown_pass_dumps_into_functions(&self, dump: PassDump) -> SplitPassDump {
        let mut pass = SplitPassDump {
            header: dump.header,
            machine: dump.machine,
            functions: HashMap::new(),
        };
        let mut func: Option<(String, Vec<String>)> = None;
        let mut is_machine_function_open = false;

        for line in dump.lines.lines() {
            let line = line.to_string();
            let ir_fn_match = self.function_define.captures(&line);
            let machine_fn_match = self.machine_function_begin.captures(&line);

            if let Some(ir_fn_match) = ir_fn_match {
                if func.is_some() {
                    let (name, lines) = func.take().unwrap();
                    pass.functions.insert(name, lines);
                }
                func = Some((ir_fn_match[1].to_string(), vec![line]));
                is_machine_function_open = false;
            } else if let Some(machine_fn_match) = machine_fn_match {
                if func.is_some() {
                    let (name, lines) = func.take().unwrap();
                    pass.functions.insert(name, lines);
                }
                func = Some((machine_fn_match[1].to_string(), vec![line]));
                is_machine_function_open = true;
            } else if line.starts_with("; Preheader:") {
                if func.is_none() {
                    func = Some(("<loop>".to_string(), vec![line]));
                }
            } else if let Some((ref mut name, ref mut lines)) = func {
                if (!is_machine_function_open && self.function_end.is_match(line.trim()))
                    || (is_machine_function_open && self.machine_function_end.is_match(line.trim()))
                {
                    lines.push(line);
                    pass.functions.insert(name.clone(), lines.clone());
                    func = None;
                } else {
                    lines.push(line);
                }
            }
        }

        if let Some((name, lines)) = func {
            pass.functions.insert(name, lines);
        }

        pass
    }

    fn breakdown_into_pass_dumps_by_function(
        &self,
        pass_dumps: Vec<SplitPassDump>,
    ) -> HashMap<String, Vec<PassDump>> {
        let mut pass_dumps_by_function = HashMap::new();
        let mut previous_function: Option<String> = None;

        for pass in pass_dumps {
            for (function_name, lines) in pass.functions {
                let name = if function_name == "<loop>" {
                    previous_function.clone().unwrap()
                } else {
                    function_name.clone()
                };
                if !pass_dumps_by_function.contains_key(&name) {
                    pass_dumps_by_function.insert(name.clone(), Vec::new());
                }
                pass_dumps_by_function
                    .get_mut(&name)
                    .unwrap()
                    .push(PassDump {
                        header: pass.header.clone(),
                        affected_function: None,
                        machine: pass.machine,
                        lines: lines.join("\n"),
                    });
                if function_name != "<loop>" {
                    previous_function = Some(name);
                }
            }
        }
        pass_dumps_by_function
    }

    fn associate_full_dumps_with_functions(
        &self,
        pass_dumps: Vec<PassDump>,
    ) -> HashMap<String, Vec<PassDump>> {
        let mut pass_dumps_by_function = HashMap::new();

        for pass in &pass_dumps {
            if let Some(ref func) = pass.affected_function {
                if !pass_dumps_by_function.contains_key(func) {
                    pass_dumps_by_function.insert(func.clone(), Vec::new());
                }
            }
        }

        pass_dumps_by_function.insert("<Full Module>".to_string(), Vec::new());
        let mut previous_function: Option<String> = None;

        for pass in pass_dumps {
            if let Some(ref func) = pass.affected_function {
                let func_name = if func.starts_with('%') {
                    previous_function.clone().unwrap()
                } else {
                    func.clone()
                };
                pass_dumps_by_function
                    .get_mut(&func_name)
                    .unwrap()
                    .push(PassDump {
                        header: format!("{} ({})", pass.header, func_name),
                        affected_function: Some(func_name.clone()),
                        machine: pass.machine,
                        lines: pass.lines.clone(),
                    });
                previous_function = Some(func_name);
            } else {
                for (_, entry) in pass_dumps_by_function.iter_mut() {
                    entry.push(PassDump {
                        header: pass.header.clone(),
                        affected_function: None,
                        machine: pass.machine,
                        lines: pass.lines.clone(),
                    });
                }
                previous_function = None;
            }
        }
        pass_dumps_by_function
    }

    fn match_pass_dumps(
        &self,
        pass_dumps_by_function: HashMap<String, Vec<PassDump>>,
    ) -> OptPipelineResults {
        let mut final_output = HashMap::new();

        for (function_name, pass_dumps) in pass_dumps_by_function {
            let mut passes: Vec<Pass> = Vec::new();
            let mut i = 0;

            while i < pass_dumps.len() {
                let mut pass = Pass {
                    name: "".to_string(),
                    machine: false,
                    after: String::new(),
                    before: String::new(),
                    ir_changed: true,
                };
                let current_dump = &pass_dumps[i];
                let next_dump = if i < pass_dumps.len() - 1 {
                    Some(&pass_dumps[i + 1])
                } else {
                    None
                };

                if current_dump.header.starts_with("IR Dump After ") {
                    pass.name = current_dump.header["IR Dump After ".len()..].to_string();
                    pass.after = current_dump.lines.clone();
                    i += 1;
                } else if current_dump.header.starts_with("IR Dump Before ") {
                    if let Some(next_dump) = next_dump {
                        if next_dump.header.starts_with("IR Dump After ") {
                            assert!(passes_match(&current_dump.header, &next_dump.header));
                            assert!(current_dump.machine == next_dump.machine);
                            pass.name = current_dump.header["IR Dump Before ".len()..].to_string();
                            pass.before = current_dump.lines.clone();
                            pass.after = next_dump.lines.clone();
                            i += 2;
                        } else {
                            pass.name = current_dump.header["IR Dump Before ".len()..].to_string();
                            pass.before = current_dump.lines.clone();
                            i += 1;
                        }
                    } else {
                        pass.name = current_dump.header["IR Dump Before ".len()..].to_string();
                        pass.before = current_dump.lines.clone();
                        i += 1;
                    }
                } else {
                    panic!("Unexpected pass header {}", current_dump.header);
                }
                pass.machine = current_dump.machine;

                if let Some(previous_pass) = passes.last() {
                    if previous_pass.machine != pass.machine {
                        pass.before = previous_pass.after.clone();
                    }
                }

                pass.ir_changed = pass.before != pass.after;
                passes.push(pass);
            }

            final_output.insert(function_name, passes);
        }
        final_output
    }

    fn breakdown_output(
        &self,
        ir: String,
        opt_pipeline_options: &OptPipelineBackendOptions,
    ) -> OptPipelineResults {
        let raw_passes = self.breakdown_output_into_pass_dumps(ir);

        if opt_pipeline_options.full_module {
            let pass_dumps_by_function = self.associate_full_dumps_with_functions(raw_passes);
            self.match_pass_dumps(pass_dumps_by_function)
        } else {
            let pass_dumps = raw_passes
                .into_iter()
                .map(|dump| self.breakdown_pass_dumps_into_functions(dump))
                .collect();
            let pass_dumps_by_function = self.breakdown_into_pass_dumps_by_function(pass_dumps);
            self.match_pass_dumps(pass_dumps_by_function)
        }
    }

    fn apply_ir_filters(
        &self,
        ir: &str,
        opt_pipeline_options: &OptPipelineBackendOptions,
    ) -> String {
        let mut filters = self.filters.clone();
        let mut line_filters = self.line_filters.clone();

        if opt_pipeline_options.filter_debug_info {
            filters.extend(self.debug_info_filters.clone());
            line_filters.extend(self.debug_info_line_filters.clone());
        }
        if opt_pipeline_options.filter_ir_metadata {
            line_filters.extend(self.metadata_line_filters.clone());
        }

        ir.lines()
            .filter(|line| filters.iter().all(|re| !re.is_match(line)))
            .map(|line| {
                let mut l = line.to_string();
                for re in &line_filters {
                    l = re.replace_all(&l, "").to_string();
                }
                line
            })
            .join("\n")
    }

    fn process(
        &self,
        output: &str,
        opt_pipeline_options: &OptPipelineBackendOptions,
    ) -> OptPipelineResults {
        let ir = output
            .lines()
            .skip_while(|line| {
                !self.ir_dump_header.is_match(line) && !self.machine_code_dump_header.is_match(line)
            })
            .join("\n");
        let preprocessed_lines = self.apply_ir_filters(&ir, opt_pipeline_options);
        self.breakdown_output(preprocessed_lines, opt_pipeline_options)
    }
}

fn passes_match(before: &str, after: &str) -> bool {
    assert!(before.starts_with("IR Dump Before "));
    assert!(after.starts_with("IR Dump After "));
    let before = &before["IR Dump Before ".len()..];
    let mut after = &after["IR Dump After ".len()..];
    if after.ends_with(" (invalidated)") {
        after = &after[..after.len() - " (invalidated)".len()];
    }
    before == after
}

fn main() -> Result<(), Box<dyn Error>> {
    let dump = std::fs::read_to_string("dump.txt").unwrap();
    let llvm_pass_dump_parser = LlvmPassDumpParser::new();
    let result = llvm_pass_dump_parser.process(
        &dump,
        &OptPipelineBackendOptions {
            filter_debug_info: true,
            filter_ir_metadata: true,
            full_module: false,
            no_discard_value_names: false,
            demangle: false,
            library_functions: false,
        },
    );

    println!("{:#?}", result);
    return Ok(());
    let pass_names = result["a"]
        .iter()
        .map(|x| x.name.clone())
        .collect::<Vec<_>>();

    // return Ok(());

    // setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // create app and run it
    let app = App::new(pass_names);
    let res = run_app(&mut terminal, app);

    // restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("{err:?}");
    }

    Ok(())
}

const PALETTES: [tailwind::Palette; 4] = [
    tailwind::BLUE,
    tailwind::EMERALD,
    tailwind::INDIGO,
    tailwind::RED,
];
const INFO_TEXT: &str =
    "(Esc) quit | (↑) move up | (↓) move down | (→) next color | (←) previous color";

const ITEM_HEIGHT: usize = 4;

struct TableColors {
    buffer_bg: Color,
    header_bg: Color,
    header_fg: Color,
    row_fg: Color,
    selected_style_fg: Color,
    normal_row_color: Color,
    alt_row_color: Color,
    footer_border_color: Color,
}

impl TableColors {
    const fn new(color: &tailwind::Palette) -> Self {
        Self {
            buffer_bg: tailwind::SLATE.c950,
            header_bg: color.c900,
            header_fg: tailwind::SLATE.c200,
            row_fg: tailwind::SLATE.c200,
            selected_style_fg: color.c400,
            normal_row_color: tailwind::SLATE.c950,
            alt_row_color: tailwind::SLATE.c900,
            footer_border_color: color.c400,
        }
    }
}

struct Data {
    name: String,
    address: String,
    email: String,
}

impl Data {
    const fn ref_array(&self) -> [&String; 3] {
        [&self.name, &self.address, &self.email]
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn address(&self) -> &str {
        &self.address
    }

    fn email(&self) -> &str {
        &self.email
    }
}

struct App {
    state: TableState,
    items: Vec<Data>,
    longest_item_lens: (u16, u16, u16), // order is (name, address, email)
    scroll_state: ScrollbarState,
    colors: TableColors,
    color_index: usize,
}

impl App {
    fn new(pass_names: Vec<String>) -> Self {
        let data_vec = generate_fake_names(pass_names);
        Self {
            state: TableState::default().with_selected(0),
            longest_item_lens: constraint_len_calculator(&data_vec),
            scroll_state: ScrollbarState::new((data_vec.len() - 1) * ITEM_HEIGHT),
            colors: TableColors::new(&PALETTES[0]),
            color_index: 0,
            items: data_vec,
        }
    }
    pub fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
    }

    pub fn previous(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * ITEM_HEIGHT);
    }

    pub fn next_color(&mut self) {
        self.color_index = (self.color_index + 1) % PALETTES.len();
    }

    pub fn previous_color(&mut self) {
        let count = PALETTES.len();
        self.color_index = (self.color_index + count - 1) % count;
    }

    pub fn set_colors(&mut self) {
        self.colors = TableColors::new(&PALETTES[self.color_index]);
    }
}

fn generate_fake_names(pass_names: Vec<String>) -> Vec<Data> {
    use fakeit::{address, contact, name};

    (0..20)
        .map(|i| {
            // let name = name::full();
            let name = pass_names.get(i).unwrap_or(&name::full()).to_owned();
            let address = format!(
                "{}\n{}, {} {}",
                address::street(),
                address::city(),
                address::state(),
                address::zip()
            );
            let email = contact::email();

            Data {
                name,
                address,
                email,
            }
        })
        .sorted_by(|a, b| a.name.cmp(&b.name))
        .collect_vec()
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, mut app: App) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, &mut app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                use KeyCode::*;
                match key.code {
                    Char('q') | Esc => return Ok(()),
                    Char('j') | Down => app.next(),
                    Char('k') | Up => app.previous(),
                    Char('l') | Right => app.next_color(),
                    Char('h') | Left => app.previous_color(),
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &mut App) {
    let rects = Layout::vertical([Constraint::Min(5), Constraint::Length(3)]).split(f.size());

    app.set_colors();

    render_table(f, app, rects[0]);

    render_scrollbar(f, app, rects[0]);

    render_footer(f, app, rects[1]);
}

fn render_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header_style = Style::default()
        .fg(app.colors.header_fg)
        .bg(app.colors.header_bg);
    let selected_style = Style::default()
        .add_modifier(Modifier::REVERSED)
        .fg(app.colors.selected_style_fg);

    let header = ["Name", "Address", "Email"]
        .into_iter()
        .map(Cell::from)
        .collect::<Row>()
        .style(header_style)
        .height(1);
    let rows = app.items.iter().enumerate().map(|(i, data)| {
        let color = match i % 2 {
            0 => app.colors.normal_row_color,
            _ => app.colors.alt_row_color,
        };
        let item = data.ref_array();
        item.into_iter()
            .map(|content| Cell::from(Text::from(format!("\n{content}\n"))))
            .collect::<Row>()
            .style(Style::new().fg(app.colors.row_fg).bg(color))
            .height(4)
    });
    let bar = " █ ";
    let t = Table::new(
        rows,
        [
            // + 1 is for padding.
            Constraint::Length(app.longest_item_lens.0 + 1),
            Constraint::Min(app.longest_item_lens.1 + 1),
            Constraint::Min(app.longest_item_lens.2),
        ],
    )
    .header(header)
    .highlight_style(selected_style)
    .highlight_symbol(Text::from(vec![
        "".into(),
        bar.into(),
        bar.into(),
        "".into(),
    ]))
    .bg(app.colors.buffer_bg)
    .highlight_spacing(HighlightSpacing::Always);
    f.render_stateful_widget(t, area, &mut app.state);
}

fn constraint_len_calculator(items: &[Data]) -> (u16, u16, u16) {
    let name_len = items
        .iter()
        .map(Data::name)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let address_len = items
        .iter()
        .map(Data::address)
        .flat_map(str::lines)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);
    let email_len = items
        .iter()
        .map(Data::email)
        .map(UnicodeWidthStr::width)
        .max()
        .unwrap_or(0);

    #[allow(clippy::cast_possible_truncation)]
    (name_len as u16, address_len as u16, email_len as u16)
}

fn render_scrollbar(f: &mut Frame, app: &mut App, area: Rect) {
    f.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None),
        area.inner(&Margin {
            vertical: 1,
            horizontal: 1,
        }),
        &mut app.scroll_state,
    );
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let info_footer = Paragraph::new(Line::from(INFO_TEXT))
        .style(Style::new().fg(app.colors.row_fg).bg(app.colors.buffer_bg))
        .centered()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::new().fg(app.colors.footer_border_color))
                .border_type(BorderType::Double),
        );
    f.render_widget(info_footer, area);
}

#[cfg(test)]
mod tests {
    use crate::Data;

    #[test]
    fn constraint_len_calculator() {
        let test_data = vec![
            Data {
                name: "Emirhan Tala".to_string(),
                address: "Cambridgelaan 6XX\n3584 XX Utrecht".to_string(),
                email: "tala.emirhan@gmail.com".to_string(),
            },
            Data {
                name: "thistextis26characterslong".to_string(),
                address: "this line is 31 characters long\nbottom line is 33 characters long"
                    .to_string(),
                email: "thisemailis40caharacterslong@ratatui.com".to_string(),
            },
        ];
        let (longest_name_len, longest_address_len, longest_email_len) =
            crate::constraint_len_calculator(&test_data);

        assert_eq!(26, longest_name_len);
        assert_eq!(33, longest_address_len);
        assert_eq!(40, longest_email_len);
    }
}
