//! Makes the code of scripts that run on players' computers hard to read
//! before a host sends it to them.
//!
//! A `LocalScript` (and any `ModuleScript` a player's computer can see) has
//! to reach that computer to run there, so its code can never be kept from
//! someone determined to take it. What this takes away is what makes taking
//! it worth it: comments and indentation go, and every local variable,
//! parameter and local function gets a short meaningless name. Globals,
//! fields, strings and numbers stay, since the script needs them to work,
//! and lines stay where they were, so errors still point at the right line.
//!
//! Nothing is guessed. The result is compiled next to the original, without
//! debug information (where local names and lines live), and only sent when
//! both give the very same bytecode, which means they run the same. If they
//! don't (Luau syntax the parser doesn't know, say), only comments and
//! indentation are taken out, checked the same way; failing that, the code
//! goes as it was.
//!
//! Why code and not bytecode: Luau doesn't check bytecode it loads, so a
//! computer that ran bytecode from whoever hosts a game could be taken over
//! by a host who sent bad bytecode. Every player's computer compiles what
//! it is sent itself instead.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use full_moon::ast::{self, Ast};
use full_moon::node::Node;
use full_moon::tokenizer::{TokenReference, TokenType};
use full_moon::visitors::Visitor;

/// How a script's code was changed before it was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shielded {
    /// Comments, indentation and local names are gone.
    Renamed,
    /// Only comments and indentation are gone.
    Stripped,
    /// Sent as it was (the checks failed, or it doesn't compile).
    AsWritten,
}

/// Code already worked out, by its hash: the code, what it became, and how.
type Cache = Mutex<HashMap<u64, (Arc<str>, Arc<str>, Shielded)>>;

/// `source` as a player's computer gets it, and how it was changed.
/// The same code is only worked out once (hosts send it to every player).
pub fn shield(source: &str) -> (Arc<str>, Shielded) {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    let key = hasher.finish();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((original, out, how)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key)
        && **original == *source
    {
        return (out.clone(), *how);
    }
    let (out, how) = work_out(source);
    let out: Arc<str> = out.into();
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    if cache.len() >= 4096 {
        cache.clear();
    }
    cache.insert(key, (source.into(), out.clone(), how));
    (out, how)
}

fn work_out(source: &str) -> (String, Shielded) {
    if source.trim().is_empty() {
        return (String::new(), Shielded::Renamed);
    }
    let Some(original) = bytecode(source) else {
        return (source.to_owned(), Shielded::AsWritten);
    };
    let Ok(tree) = full_moon::parse_fallible(source, full_moon::LuaVersion::luau()).into_result() else {
        return (source.to_owned(), Shielded::AsWritten);
    };
    if let Some(renames) = local_names(&tree)
        && let Some(renamed) = write(source, &renames)
        && bytecode(&renamed).as_ref() == Some(&original)
    {
        return (renamed, Shielded::Renamed);
    }
    if let Some(stripped) = write(source, &HashMap::new())
        && bytecode(&stripped).as_ref() == Some(&original)
    {
        return (stripped, Shielded::Stripped);
    }
    (source.to_owned(), Shielded::AsWritten)
}

/// The code's bytecode with no debug information: only what it does,
/// nothing about how it was written.
fn bytecode(source: &str) -> Option<Vec<u8>> {
    mlua::chunk::Compiler::new().set_debug_level(0).compile(source).ok()
}

/// The code again from its tokens: comments dropped (except `--!` lines,
/// which change how Luau compiles it), the space between tokens cut to one
/// space or the line breaks it had, and the tokens at `renames` (by where
/// they start) given their new names.
fn write(source: &str, renames: &HashMap<usize, String>) -> Option<String> {
    use full_moon::tokenizer::{Lexer, LexerResult};
    let LexerResult::Ok(tokens) = Lexer::new(source, full_moon::LuaVersion::luau()).collect() else {
        return None;
    };
    let mut out = String::with_capacity(source.len() / 2);
    let mut last_end = 0usize;
    for token in &tokens {
        let (start, end) = (token.start_position().bytes(), token.end_position().bytes());
        match token.token_type() {
            TokenType::Whitespace { .. } | TokenType::MultiLineComment { .. } | TokenType::Eof => continue,
            TokenType::SingleLineComment { comment } if !comment.starts_with('!') => continue,
            TokenType::Shebang { .. } => continue,
            _ => {}
        }
        gap(&mut out, source.get(last_end..start)?);
        match renames.get(&start) {
            Some(name) => out.push_str(name),
            None => out.push_str(source.get(start..end)?),
        }
        last_end = end;
    }
    gap(&mut out, source.get(last_end..)?);
    Some(out)
}

/// What goes between two tokens: the line breaks there were, else one
/// space if there was anything at all.
fn gap(out: &mut String, between: &str) {
    let lines = between.matches('\n').count();
    if lines > 0 {
        out.extend(std::iter::repeat_n('\n', lines));
    } else if !between.is_empty() {
        out.push(' ');
    }
}

/// Where each local's name appears (declared and used), with its new
/// name; `None` if the code uses something this doesn't follow.
fn local_names(tree: &Ast) -> Option<HashMap<usize, String>> {
    let mut taken = HashSet::new();
    for token in tree.nodes().tokens() {
        if let TokenType::Identifier { identifier } = token.token_type() {
            taken.insert(identifier.to_string());
        }
    }
    let mut scopes = Scopes { taken, ..Scopes::default() };
    scopes.visit_ast(tree);
    (!scopes.unsure).then_some(scopes.renames)
}

/// Words a local can't be called, even where Luau only sometimes means them.
const KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while", "continue", "type", "export",
    "typeof", "const", "self",
];

#[derive(Default)]
struct Scopes {
    /// Innermost last: each name declared there and its new name.
    stack: Vec<HashMap<String, String>>,
    /// Names to declare when a block starts (a loop's variables), by the block.
    pending: HashMap<*const ast::Block, Vec<(usize, String)>>,
    /// Blocks whose names stay until their statement ends (`repeat ... until`).
    held: HashSet<*const ast::Block>,
    renames: HashMap<usize, String>,
    /// Every name in the code, which a new name mustn't be.
    taken: HashSet<String>,
    next: usize,
    unsure: bool,
}

impl Scopes {
    fn fresh(&mut self) -> String {
        const FIRST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
        const REST: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";
        loop {
            let mut n = self.next;
            self.next += 1;
            let mut name = String::new();
            name.push(FIRST[n % FIRST.len()] as char);
            n /= FIRST.len();
            while n > 0 {
                n -= 1;
                name.push(REST[n % REST.len()] as char);
                n /= REST.len();
            }
            if !self.taken.contains(&name) && !KEYWORDS.contains(&name.as_str()) {
                return name;
            }
        }
    }

    fn declare(&mut self, token: &TokenReference) {
        let TokenType::Identifier { identifier } = token.token_type() else { return };
        let name = self.fresh();
        self.renames.insert(token.token().start_position().bytes(), name.clone());
        match self.stack.last_mut() {
            Some(scope) => {
                scope.insert(identifier.to_string(), name);
            }
            None => self.unsure = true,
        }
    }

    fn refer(&mut self, token: &TokenReference) {
        let TokenType::Identifier { identifier } = token.token_type() else { return };
        if let Some(name) = self.stack.iter().rev().find_map(|s| s.get(identifier.as_str())) {
            self.renames.insert(token.token().start_position().bytes(), name.clone());
        }
    }
}

impl Visitor for Scopes {
    fn visit_block(&mut self, block: &ast::Block) {
        self.stack.push(HashMap::new());
        for (at, old) in self.pending.remove(&(block as *const _)).unwrap_or_default() {
            let new = self.renames[&at].clone();
            if let Some(scope) = self.stack.last_mut() {
                scope.insert(old, new);
            }
        }
    }

    fn visit_block_end(&mut self, block: &ast::Block) {
        if !self.held.contains(&(block as *const _)) {
            self.stack.pop();
        }
    }

    fn visit_repeat(&mut self, repeat: &ast::Repeat) {
        self.held.insert(repeat.block() as *const _);
    }

    fn visit_repeat_end(&mut self, repeat: &ast::Repeat) {
        if self.held.remove(&(repeat.block() as *const _)) {
            self.stack.pop();
        }
    }

    fn visit_local_assignment_end(&mut self, local: &ast::LocalAssignment) {
        // Declared after the values, which see the names from outside.
        for name in local.names() {
            self.declare(name);
        }
    }

    fn visit_local_function(&mut self, function: &ast::LocalFunction) {
        // Seen inside its own body, so it can call itself.
        self.declare(function.name());
    }

    fn visit_function_body(&mut self, body: &ast::FunctionBody) {
        self.stack.push(HashMap::new());
        for parameter in body.parameters() {
            if let ast::Parameter::Name(name) = parameter {
                self.declare(name);
            }
        }
    }

    fn visit_function_body_end(&mut self, _: &ast::FunctionBody) {
        self.stack.pop();
    }

    fn visit_numeric_for(&mut self, numeric: &ast::NumericFor) {
        self.later(numeric.block(), std::iter::once(numeric.index_variable()));
    }

    fn visit_generic_for(&mut self, generic: &ast::GenericFor) {
        self.later(generic.block(), generic.names().iter());
    }

    fn visit_function_declaration(&mut self, declaration: &ast::FunctionDeclaration) {
        if let Some(first) = declaration.name().names().iter().next() {
            self.refer(first);
        }
    }

    fn visit_var(&mut self, var: &ast::Var) {
        if let ast::Var::Name(name) = var {
            self.refer(name);
        }
    }

    fn visit_prefix(&mut self, prefix: &ast::Prefix) {
        if let ast::Prefix::Name(name) = prefix {
            self.refer(name);
        }
    }

    fn visit_const_assignment_end(&mut self, constant: &ast::luau::ConstAssignment) {
        for name in constant.names() {
            self.declare(name);
        }
    }

    fn visit_const_function(&mut self, function: &ast::luau::ConstFunction) {
        self.declare(function.name());
    }

    // A newer way to make a name, not followed here: the code keeps its names.
    fn visit_if_condition_binding(&mut self, _: &ast::luau::IfConditionBinding) {
        self.unsure = true;
    }
}

impl Scopes {
    /// A loop's variables, declared when its body starts: the loop's own
    /// values (`for i = 1, n`) still see the names from outside.
    fn later<'a>(&mut self, block: &ast::Block, names: impl Iterator<Item = &'a TokenReference>) {
        let mut list = Vec::new();
        for token in names {
            if let TokenType::Identifier { identifier } = token.token_type() {
                let new = self.fresh();
                let at = token.token().start_position().bytes();
                self.renames.insert(at, new);
                list.push((at, identifier.to_string()));
            }
        }
        self.pending.insert(block as *const _, list);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(source: &str) -> String {
        let (out, how) = shield(source);
        assert_eq!(how, Shielded::Renamed, "{source}\n---\n{out}");
        out.to_string()
    }

    #[test]
    fn comments_and_local_names_go_but_lines_and_globals_stay() {
        let out = check(
            "-- The coin counter\nlocal coinsCollected = 0 -- how many\nlocal function addCoins(amount)\n\tcoinsCollected += amount\n\tprint(\"coins\", coinsCollected)\nend\n\naddCoins(5)\n",
        );
        assert!(!out.contains("coin counter") && !out.contains("how many"), "{out}");
        assert!(
            !out.contains("coinsCollected") && !out.contains("addCoins") && !out.contains("amount"),
            "{out}"
        );
        assert!(out.contains("print(\"coins\""), "{out}");
        assert_eq!(out.lines().count(), 8, "{out}");
    }

    #[test]
    fn scopes_shadowing_loops_and_repeat_are_followed() {
        check(
            r#"
local print = print
local x = 1
do local x = x + 1; print(x) end
for i = x, 10 do local i = i * 2; print(i) end
for k, v in pairs({ a = 1, b = 2 }) do print(k, v, x) end
local n = 0
repeat local done = n > 3; n += 1 until done
local t = { x = x, ["y"] = 2 }
function t.go(self, a, ...) return self.x + a, ... end
function t:run(b) return self.x + b end
local function fact(n) if n <= 1 then return 1 end return n * fact(n - 1) end
local s = `value {x} and {fact(3)}`
local y: number = if x > 0 then x else -x
type Point = { x: number }
local p: Point = { x = y }
while p.x > 100 do p.x -= 1 end
local f = function(x) return function() return x end end
return f, s, t, p
"#,
        );
    }

    #[test]
    fn hot_comments_stay() {
        let out = check("--!native\n--!optimize 2\nlocal a = 1\nreturn a\n");
        assert!(out.starts_with("--!native\n--!optimize 2\n"), "{out}");
    }

    #[test]
    fn code_that_does_not_compile_goes_as_it_was() {
        assert_eq!(shield("local = 1").1, Shielded::AsWritten);
        assert_eq!(&*shield("local = 1").0, "local = 1");
    }

    #[test]
    fn the_shielded_code_runs_the_same() {
        let source = "local total = 0\nfor index = 1, 10 do\n  local square = index * index -- squares\n  total += square\nend\nlocal function twice(value) return value * 2 end\nreturn twice(total)\n";
        let lua = mlua::Lua::new();
        let before: i64 = lua.load(source).eval().unwrap();
        let after: i64 = lua.load(&*check(source)).eval().unwrap();
        assert_eq!(before, after);
    }
}
