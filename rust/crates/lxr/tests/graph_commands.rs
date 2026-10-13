//! The graph commands and the search command, on a small Python project, with exact
//! expected output.

mod common;
use common::{expect, Env};

/// Builds the index once so that later runs print nothing on standard error.
fn built() -> Env {
    let env = Env::with_project();
    let run = env.run(&["refresh"]);
    assert_eq!(run.code, 0, "{}", run.err);
    env
}

#[test]
fn def_prints_one_row_per_definition_and_not_found() {
    let env = built();
    let run = env.run(&["def", "load_config", "Store.get", "nope", "Store"]);
    expect(
        &run,
        "load_config app/core.py:4-6 (path)\n\
         Store.get app/core.py:30-32 (self, key)\n\
         nope: not found\n\
         Store app/core.py:24-37 class Store\n",
    );
    assert_eq!(run.err, "");
}

#[test]
fn def_accepts_the_path_form_of_a_name() {
    let env = built();
    expect(&env.run(&["def", "app/util.py::slugify"]), "slugify app/util.py:1-3 (text)\n");
    expect(&env.run(&["def", "core.py:read_text"]), "read_text app/core.py:9-12 (path)\n");
}

#[test]
fn callers_lists_callers_with_lines_a_module_level_caller_and_zero() {
    let env = built();
    expect(
        &env.run(&["callers", "main", "load_config", "unused_helper"]),
        "1 caller of main:\n\
         <module> app/cli.py @17\n\
         2 callers of load_config:\n\
         main app/cli.py:6-9 @8\n\
         Store.reload app/core.py:34-37 @36\n\
         0 callers of unused_helper\n",
    );
}

#[test]
fn callers_puts_loose_callers_under_by_name_only() {
    let env = built();
    expect(
        &env.run(&["callers", "get"]),
        "1 caller of get:\n\
         Store.reload app/core.py:34-37 @37\n\
         by name only:\n\
         report app/cli.py:12-14 @14\n\
         Store.get app/core.py:30-32 @32\n",
    );
}

#[test]
fn callers_of_a_name_with_several_definitions_make_one_block_each() {
    let env = built();
    env.write("app/other.py", "def helper():\n    return 1\n");
    env.write("app/third.py", "def helper():\n    return 2\n\n\ndef again():\n    return helper()\n");
    let run = env.run(&["callers", "helper"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.starts_with("# helper at app/other.py:1\n0 callers of helper\n# helper at app/third.py:1\n"), "{}", run.out);
    assert!(run.out.contains("again app/third.py:5-6 @6"), "{}", run.out);
}

#[test]
fn about_prints_doc_callers_and_calls() {
    let env = built();
    expect(
        &env.run(&["about", "load_config", "unused_helper", "Store"]),
        "load_config app/core.py:4-6 (path)\n\
         \x20 doc: Read the configuration file at path.\n\
         \x20 2 callers:\n\
         \x20   main app/cli.py:6-9 @8\n\
         \x20   Store.reload app/core.py:34-37 @36\n\
         \x20 calls: read_text, parse_text\n\
         unused_helper app/util.py:6-8 ()\n\
         \x20 doc: Nothing calls this.\n\
         \x20 0 callers\n\
         Store app/core.py:24-37 class Store\n\
         \x20 doc: Keeps settings in memory.\n\
         \x20 1 caller:\n\
         \x20   main app/cli.py:6-9 @8\n",
    );
}

#[test]
fn affected_walks_back_over_calls_nearest_first() {
    let env = built();
    expect(
        &env.run(&["affected", "read_text"]),
        "3 affected by read_text (depth 2):\n\
         \x20 d1 load_config app/core.py:4-6 via calls @6\n\
         \x20 d2 main app/cli.py:6-9 via calls @8\n\
         \x20 d2 Store.reload app/core.py:34-37 via calls @36\n",
    );
    expect(
        &env.run(&["affected", "read_text", "--depth", "1"]),
        "1 affected by read_text (depth 1):\n  d1 load_config app/core.py:4-6 via calls @6\n",
    );
    expect(&env.run(&["affected", "unused_helper"]), "0 affected by unused_helper\n");
}

#[test]
fn path_prints_hops_and_the_no_path_answer() {
    let env = built();
    expect(
        &env.run(&["path", "main", "read_text"]),
        "main -> read_text: 2 hops\n\
         \x20 main app/cli.py:6 --calls--> load_config app/core.py:4\n\
         \x20 load_config app/core.py:4 --calls--> read_text app/core.py:9\n",
    );
    expect(&env.run(&["path", "slugify", "main"]), "no path between slugify and main\n");
    expect(&env.run(&["path", "read_text", "load_config"]), "read_text -> load_config: 1 hops\n  read_text app/core.py:9 <--calls-- load_config app/core.py:4\n");
}

#[test]
fn hubs_lists_the_most_connected_definitions() {
    let env = built();
    expect(
        &env.run(&["hubs", "-k", "4"]),
        "load_config app/core.py:4-6  5 links\n\
         Store app/core.py:24-37  5 links\n\
         main app/cli.py:6-9  4 links\n\
         Store.reload app/core.py:34-37  3 links\n",
    );
}

#[test]
fn explain_prints_container_members_and_callees() {
    let env = built();
    expect(
        &env.run(&["explain", "Store"]),
        "Store app/core.py:24-37 class Store\n\
         \x20 doc: Keeps settings in memory.\n\
         \x20 members: __init__, get, reload\n\
         \x20 1 caller:\n\
         \x20   main app/cli.py:6-9 @8\n",
    );
    expect(
        &env.run(&["explain", "load_config"]),
        "load_config app/core.py:4-6 (path)\n\
         \x20 doc: Read the configuration file at path.\n\
         \x20 2 callers:\n\
         \x20   main app/cli.py:6-9 @8\n\
         \x20   Store.reload app/core.py:34-37 @36\n\
         \x20 calls:\n\
         \x20   read_text app/core.py:9-12 (path)\n\
         \x20   parse_text app/core.py:15-21 (text)\n",
    );
    let method = env.run(&["explain", "Store.get"]);
    assert!(method.out.contains("  in: Store\n"), "{}", method.out);
}

#[test]
fn query_and_map_describe_the_neighbourhood_and_the_groups() {
    let env = built();
    let query = env.run(&["query", "how are settings loaded", "--budget", "400"]);
    assert_eq!(query.code, 0, "{}", query.err);
    let first = query.out.lines().next().unwrap();
    assert!(first.starts_with("query: ") && first.contains(" nodes from "), "{first}");
    assert!(query.out.contains("load_config app/core.py:4-6\n"), "{}", query.out);
    assert!(query.out.contains("  main --calls--> load_config @8\n"), "{}", query.out);

    expect(
        &env.run(&["map"]),
        "group 1 (4 symbols): Store, Store.reload, Store.get, Store.__init__; files: app/core.py\n\
         group 2 (3 symbols): load_config, read_text, parse_text; files: app/core.py\n\
         group 3 (2 symbols): main, report; files: app/cli.py\n",
    );
    let tight = env.run(&["map", "--budget", "1"]);
    assert_eq!(tight.out.lines().count(), 1, "{}", tight.out);
}

#[test]
fn names_that_are_not_found_answer_with_exit_zero() {
    let env = built();
    for command in ["def", "callers", "about", "explain", "affected"] {
        let run = env.run(&[command, "no_such_name"]);
        expect(&run, "no_such_name: not found\n");
    }
    let run = env.run(&["path", "no_such_name", "main"]);
    expect(&run, "no_such_name: not found\n");
}

#[test]
fn find_answers_in_each_mode() {
    let env = built();
    let top = "load_config app/core.py:4-6 (path)\n  Read the configuration file at path.\n";
    expect(&env.run(&["find", "-k", "1", "read", "the", "configuration", "file"]), top);
    expect(&env.run(&["find", "-k", "1", "--words", "read", "the", "configuration", "file"]), top);
    // The tests hide the embedding model, so a search by meaning alone finds nothing.
    expect(&env.run(&["find", "--meaning", "read", "the", "configuration", "file"]), "none\n");
    expect(&env.run(&["find", "qqqzzzxxx"]), "none\n");
    let several = env.run(&["find", "settings"]);
    assert_eq!(several.code, 0);
    assert!(several.out.lines().count() >= 2, "{}", several.out);
}

#[test]
fn a_hit_outside_any_definition_prints_its_span_only() {
    let env = built();
    env.write("app/script.py", "import os\nimport sys\n\nprint('PGR_TEST_ONLY_WORD', os.name, sys.argv)\n");
    expect(&env.run(&["find", "-k", "1", "PGR_TEST_ONLY_WORD"]), "app/script.py:1-4\n");
}

#[test]
fn ask_answers_several_kinds_under_their_own_headers() {
    let env = built();
    expect(
        &env.run(&["ask", "def:load_config", "callers:main", "about:Store.get", "affected:read_text", "path:main>read_text"]),
        "== def:load_config\n\
         load_config app/core.py:4-6 (path)\n\
         == callers:main\n\
         1 caller of main:\n\
         <module> app/cli.py @17\n\
         == about:Store.get\n\
         Store.get app/core.py:30-32 (self, key)\n\
         \x20 doc: Look up one setting.\n\
         \x20 1 caller:\n\
         \x20   Store.reload app/core.py:34-37 @37\n\
         == affected:read_text\n\
         3 affected by read_text (depth 2):\n\
         \x20 d1 load_config app/core.py:4-6 via calls @6\n\
         \x20 d2 main app/cli.py:6-9 via calls @8\n\
         \x20 d2 Store.reload app/core.py:34-37 via calls @36\n\
         == path:main>read_text\n\
         main -> read_text: 2 hops\n\
         \x20 main app/cli.py:6 --calls--> load_config app/core.py:4\n\
         \x20 load_config app/core.py:4 --calls--> read_text app/core.py:9\n",
    );
    let mixed = env.run(&["ask", "explain:Store", "find:read the configuration file"]);
    assert_eq!(mixed.code, 0, "{}", mixed.err);
    assert!(mixed.out.starts_with("== explain:Store\nStore app/core.py:24-37 class Store\n"), "{}", mixed.out);
    assert!(mixed.out.contains("== find:read the configuration file\nload_config app/core.py:4-6 (path)\n"), "{}", mixed.out);
}

#[test]
fn ask_with_an_unknown_kind_is_a_usage_error() {
    let env = built();
    for bad in ["bogus:x", "nokind", "def:", "path:onlyone"] {
        let run = env.run(&["ask", bad]);
        assert_eq!(run.code, 2, "{bad}: {}", run.err);
        assert_eq!(run.out, "");
        assert!(run.err.starts_with("pgr: "), "{}", run.err);
    }
}

#[test]
fn usage_errors_exit_with_two() {
    let env = built();
    for args in [
        vec![],
        vec!["bogus"],
        vec!["def"],
        vec!["find"],
        vec!["find", "x", "--words", "--meaning"],
        vec!["path", "only"],
        vec!["recall", "x", "--budget", "huge"],
        vec!["hubs", "-k", "0"],
        vec!["status", "extra"],
        vec!["--engines", "nonsense", "status"],
    ] {
        let run = env.run(&args);
        assert_eq!(run.code, 2, "{args:?}: {}", run.err);
        assert_eq!(run.out, "", "{args:?}");
        assert!(!run.err.is_empty(), "{args:?}");
    }
}

#[test]
fn no_command_prints_the_short_help_on_standard_error() {
    let env = Env::new();
    let run = env.command().output().unwrap();
    assert_eq!(run.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run.stderr).contains("usage: pgr"));
    assert!(run.stdout.is_empty());
}

#[test]
fn help_and_version_answer_on_standard_output() {
    let env = Env::new();
    let help = env.run(&["help"]);
    assert_eq!(help.code, 0);
    assert!(help.out.contains("usage: pgr") && help.out.contains("callers NAME..."), "{}", help.out);
    for flag in ["--help", "-h"] {
        assert_eq!(env.run(&[flag]).out, help.out);
    }
    let version = env.run(&["--version"]);
    assert_eq!(version.out, format!("pgr {}\n", env!("CARGO_PKG_VERSION")));
    assert_eq!(env.run(&["-V"]).out, version.out);
}

#[test]
fn engines_flag_and_variable_turn_commands_off() {
    let env = built();
    let off = env.run(&["--engines", "graph,memory", "find", "x"]);
    assert_eq!((off.code, off.out.as_str(), off.err.as_str()), (2, "", "pgr: the search engine is off\n"));
    let off = env.run(&["--engines", "search", "def", "main"]);
    assert_eq!((off.code, off.err.as_str()), (2, "pgr: the graph engine is off\n"));
    let off = env.run(&["--engines", "graph", "recall", "x"]);
    assert_eq!((off.code, off.err.as_str()), (2, "pgr: the memory engine is off\n"));
    let off = env.run(&["--engines", "graph", "ask", "find:x"]);
    assert_eq!((off.code, off.err.as_str()), (2, "pgr: the search engine is off\n"));

    let by_variable = env.run_with(&["def", "main"], &[("PGR_ENGINES", "search")]);
    assert_eq!((by_variable.code, by_variable.err.as_str()), (2, "pgr: the graph engine is off\n"));
    // The flag wins over the variable.
    let flag_wins = env.run_with(&["--engines", "graph", "def", "main"], &[("PGR_ENGINES", "search")]);
    expect(&flag_wins, "main app/cli.py:6-9 (argv)\n");
    // Commands that belong to no engine still work.
    assert_eq!(env.run(&["--engines", "memory", "status"]).code, 0);
}

#[test]
fn the_root_is_found_from_the_current_folder() {
    let env = Env::with_project();
    std::fs::create_dir_all(env.proj.join(".git")).unwrap();
    let run = env.run_in(&env.proj.join("app"), &["def", "slugify"]);
    expect(&run, "slugify app/util.py:1-3 (text)\n");
    assert!(run.err.starts_with("pgr: index built in "), "{}", run.err);
}

#[test]
fn a_root_that_is_not_a_folder_is_a_usage_error() {
    let env = Env::new();
    let run = env.run_at(&env.base.join("missing"), &["status"], &[]);
    assert_eq!(run.code, 2);
    assert!(run.err.contains("is not a folder"), "{}", run.err);
}
