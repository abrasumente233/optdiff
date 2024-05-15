use itertools::Itertools;
use regex::Regex;
use std::collections::HashMap;

#[derive(Debug)]
pub struct Pass {
    pub name: String,
    pub machine: bool,
    pub after: String,
    pub before: String,
    pub ir_changed: bool,
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

pub struct LlvmPassDumpParser {
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

pub fn process(dump: &str) -> OptPipelineResults {
    let llvm_pass_dump_parser = LlvmPassDumpParser::new();
    llvm_pass_dump_parser.process(
        dump,
        &OptPipelineBackendOptions {
            filter_debug_info: true,
            filter_ir_metadata: true,
            full_module: false,
            no_discard_value_names: false,
            demangle: false,
            library_functions: false,
        },
    )
}
