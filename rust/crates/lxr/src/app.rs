//! Running a parsed command against a run's state. The command line and the MCP server
//! both come through here, so a tool gives the same text as the command it stands for.

use crate::cli::Command;
use crate::ctx::Ctx;
use crate::fail::{Fail, Res};
use crate::index::status;
use crate::{ask, findcmd, graphcmd, memcmd, pack};
use lxr_text::Embedder;

/// The text a command prints on standard output.
pub fn dispatch(ctx: &mut Ctx, command: &Command) -> Res<String> {
    if let Some(engine) = command.engine() {
        ctx.engines.require(engine)?;
    }
    let mut out = String::new();
    match command {
        Command::Refresh { force } => {
            ctx.check_folder()?;
            let done = ctx.run_refresh(*force)?;
            // Said once per `pgr refresh`, not on every automatic refresh before a query.
            if done.words_only || (!done.rebuilt && Embedder::locate().is_none()) {
                crate::say("pgr: no embedding model found, search uses words only");
            }
            ln!(out, "{}", status::line(ctx)?);
        }
        Command::Status => ln!(out, "{}", status::line(ctx)?),
        Command::Def { names } => {
            ctx.need_graph()?;
            graphcmd::def(ctx.graph()?, names, &mut out);
        }
        Command::Callers { names } => {
            ctx.need_graph()?;
            graphcmd::callers(ctx.graph()?, names, &mut out);
        }
        Command::About { names } => {
            ctx.need_graph()?;
            graphcmd::about(ctx.graph()?, names, &mut out);
        }
        Command::Affected { name, depth } => {
            ctx.need_graph()?;
            graphcmd::affected(ctx.graph()?, name, *depth, &mut out);
        }
        Command::Path { from, to } => {
            ctx.need_graph()?;
            graphcmd::path(ctx.graph()?, from, to, &mut out);
        }
        Command::Hubs { k } => {
            ctx.need_graph()?;
            graphcmd::hubs(ctx.graph()?, *k, &mut out);
        }
        Command::Explain { name } => {
            ctx.need_graph()?;
            graphcmd::explain(ctx.graph()?, name, &mut out);
        }
        Command::Query { question, budget } => {
            ctx.need_graph()?;
            graphcmd::query(ctx.graph()?, question, *budget, &mut out);
        }
        Command::Map { budget } => {
            ctx.need_graph()?;
            graphcmd::map(ctx.graph()?, *budget, &mut out);
        }
        Command::Find(args) => {
            ctx.need_search()?;
            findcmd::run(ctx, args, &mut out)?;
        }
        Command::Retain(args) => memcmd::retain(ctx, args, &mut out)?,
        Command::Recall(args) => memcmd::recall(ctx, args, &mut out)?,
        Command::Reflect { query } => memcmd::reflect(ctx, query, &mut out)?,
        Command::Memory(sub) => memcmd::memory(ctx, sub, &mut out)?,
        Command::Ask { questions } => out = ask::run(ctx, &ask::parse(questions)?)?,
        Command::Pack { task, budget } => out = pack::run(ctx, task, *budget)?,
        Command::Mcp | Command::Help | Command::Version => {
            return Err(Fail::usage("this command does not run inside another command"));
        }
    }
    Ok(out)
}
