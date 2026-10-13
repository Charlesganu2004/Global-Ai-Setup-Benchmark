//! `path` across every definition of a name, a function that calls itself, and a pack
//! that stays inside a tiny budget.

mod common;
use common::{expect, Env};

/// Five files that define `foo`; only the last one (by path) calls `bar`.
fn five_foos() -> Env {
    let env = Env::new();
    for n in 1..=4 {
        env.write(&format!("pkg/a{n}.py"), "def foo():\n    return 0\n");
    }
    env.write("pkg/a5.py", "from pkg.lib import bar\n\n\ndef foo():\n    return bar()\n");
    env.write("pkg/lib.py", "def bar():\n    return 1\n");
    env
}

#[test]
fn path_looks_at_every_definition_of_a_name_not_just_the_first_four() {
    let env = five_foos();
    let run = env.run(&["path", "foo", "bar"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.starts_with("foo -> bar: 1 hops\n"), "{}", run.out);
    assert!(run.out.contains("pkg/a5.py:4 --calls--> bar pkg/lib.py:1"), "{}", run.out);
    // The same through ask.
    let asked = env.run(&["ask", "path:foo>bar"]);
    assert!(asked.out.contains("pkg/a5.py:4 --calls--> bar pkg/lib.py:1"), "{}", asked.out);
}

#[test]
fn path_picks_the_shortest_chain_over_all_pairs_of_definitions() {
    let env = Env::new();
    // The first definitions of `start` are far from `goal`; the last one is a neighbour.
    env.write("m/b1.py", "def start():\n    return step_one()\n\n\ndef step_one():\n    return step_two()\n\n\ndef step_two():\n    return goal()\n");
    for n in 2..=5 {
        env.write(&format!("m/b{n}.py"), "def start():\n    return 0\n");
    }
    env.write("m/b6.py", "from m.z import goal\n\n\ndef start():\n    return goal()\n");
    env.write("m/z.py", "def goal():\n    return 1\n");
    let run = env.run(&["path", "start", "goal"]);
    assert!(run.out.starts_with("start -> goal: 1 hops\n"), "{}", run.out);
    assert!(run.out.contains("m/b6.py:4 --calls--> goal m/z.py:1"), "{}", run.out);
}

#[test]
fn a_function_that_calls_itself_is_listed_as_its_own_caller() {
    let env = Env::new();
    env.write(
        "rec.py",
        "def fact(n):\n    if n <= 1:\n        return 1\n    return n * fact(n - 1)\n\n\ndef run():\n    return fact(5)\n",
    );
    expect(
        &env.run(&["callers", "fact"]),
        "2 callers of fact:\nfact rec.py:1-4 @4 (calls itself)\nrun rec.py:7-8 @8\n",
    );
    let about = env.run(&["about", "fact"]);
    assert_eq!(about.code, 0, "{}", about.err);
    assert!(
        about.out.contains("  2 callers:\n    fact rec.py:1-4 @4 (calls itself)\n    run rec.py:7-8 @8\n"),
        "{}",
        about.out
    );
    let explain = env.run(&["explain", "fact"]);
    assert!(
        explain.out.contains("  2 callers:\n    fact rec.py:1-4 @4 (calls itself)\n    run rec.py:7-8 @8\n"),
        "{}",
        explain.out
    );
    // The same rows come back through ask.
    let asked = env.run(&["ask", "callers:fact"]);
    assert!(asked.out.starts_with("== callers:fact\n2 callers of fact:\nfact rec.py:1-4 @4 (calls itself)\n"), "{}", asked.out);
}

#[test]
fn a_function_that_only_calls_itself_has_one_caller() {
    let env = Env::new();
    env.write("loop.py", "def spin(n):\n    return spin(n + 1)\n");
    expect(&env.run(&["callers", "spin"]), "1 caller of spin:\nspin loop.py:1-2 @2 (calls itself)\n");
    let about = env.run(&["about", "spin"]);
    assert!(about.out.contains("  1 caller:\n    spin loop.py:1-2 @2 (calls itself)\n"), "{}", about.out);
}

#[test]
fn a_function_that_does_not_call_itself_is_not_marked() {
    let env = Env::with_project();
    let run = env.run(&["callers", "read_text"]);
    assert!(!run.out.contains("calls itself"), "{}", run.out);
}

#[test]
fn a_tiny_pack_budget_is_not_overrun() {
    let env = Env::with_project();
    let task = "change how load_config reads the settings file";
    for budget in [3usize, 6, 9] {
        let run = env.run(&["pack", task, "--budget", &budget.to_string()]);
        assert_eq!(run.code, 0, "{}", run.err);
        let mut spent = 0usize;
        let mut total = 0usize;
        for line in run.out.lines().chain(std::iter::once("== end")) {
            if line.starts_with("== ") {
                total += spent;
                spent = 0;
            } else {
                spent += (line.len() + 3) / 4 + 1;
            }
        }
        // A line that is cut costs its tokens and the line break: one token more than its share,
        // once in each of the three sections after the names.
        assert!(total <= budget + 3, "{total} tokens at budget {budget}: {}", run.out);
    }
}
