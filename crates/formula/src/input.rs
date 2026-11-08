//! Localized aliases accepted at user-entry boundaries; file syntax stays English.

use crate::{Expr, ParseError, parser};

/// Formula-entry syntax selected by the interface language. Other interface languages
/// currently use the canonical English function table until a translation table is supplied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FormulaLocale {
    #[default]
    En,
    Es,
    Pt,
}

const SPANISH: &[(&str, &str)] = &[
    ("SUM", "SUMA"),
    ("AVERAGE", "PROMEDIO"),
    ("IF", "SI"),
    ("COUNT", "CONTAR"),
    ("SUMIF", "SUMAR.SI"),
    ("COUNTIF", "CONTAR.SI"),
    ("VLOOKUP", "BUSCARV"),
    ("TODAY", "HOY"),
    // Financial block (#1986): the loan-schedule and investment staples.
    ("NPV", "VNA"),
    ("IRR", "TIR"),
    ("PMT", "PAGO"),
    ("PV", "VA"),
    ("FV", "VF"),
    ("TRUE", "VERDADERO"),
    ("FALSE", "FALSO"),
];

const PORTUGUESE: &[(&str, &str)] = &[
    // Math and trigonometry.
    ("SUM", "SOMA"),
    ("SUMPRODUCT", "SOMARPRODUTO"),
    ("SUMSQ", "SOMAQUAD"),
    ("PRODUCT", "MULT"),
    ("ROUND", "ARRED"),
    ("ROUNDUP", "ARREDONDAR.PARA.CIMA"),
    ("ROUNDDOWN", "ARREDONDAR.PARA.BAIXO"),
    ("MROUND", "MARRED"),
    ("CEILING", "TETO"),
    ("FLOOR", "ARREDMULTB"),
    ("TRUNC", "TRUNCAR"),
    ("QUOTIENT", "QUOCIENTE"),
    ("SIGN", "SINAL"),
    ("SQRT", "RAIZ"),
    ("POWER", "POTÊNCIA"),
    ("RAND", "ALEATÓRIO"),
    ("RANDBETWEEN", "ALEATÓRIOENTRE"),
    ("FACT", "FATORIAL"),
    ("GCD", "MDC"),
    ("LCM", "MMC"),
    ("EVEN", "PAR"),
    ("ODD", "ÍMPAR"),
    ("SIN", "SEN"),
    ("ASIN", "ASEN"),
    ("DEGREES", "GRAUS"),
    ("RADIANS", "RADIANOS"),
    ("ROMAN", "ROMANO"),
    ("ARABIC", "ARÁBICO"),
    ("MMULT", "MATRIZ.MULT"),
    ("MDETERM", "MATRIZ.DETERM"),
    // Conditional aggregation and statistics.
    ("SUMIF", "SOMASE"),
    ("SUMIFS", "SOMASES"),
    ("COUNTIF", "CONT.SE"),
    ("COUNTIFS", "CONT.SES"),
    ("AVERAGEIF", "MÉDIASE"),
    ("AVERAGEIFS", "MÉDIASES"),
    ("MAXIFS", "MÁXIMOSES"),
    ("MINIFS", "MÍNIMOSES"),
    ("AVERAGE", "MÉDIA"),
    ("AVERAGEA", "MÉDIAA"),
    ("MEDIAN", "MED"),
    ("MODE", "MODO"),
    ("STDEV", "DESVPAD"),
    ("STDEV.S", "DESVPAD.A"),
    ("STDEV.P", "DESVPAD.P"),
    ("STDEVP", "DESVPADP"),
    ("VAR.S", "VAR.A"),
    ("MIN", "MÍNIMO"),
    ("MINA", "MÍNIMOA"),
    ("MAX", "MÁXIMO"),
    ("MAXA", "MÁXIMOA"),
    ("LARGE", "MAIOR"),
    ("SMALL", "MENOR"),
    ("RANK", "ORDEM"),
    ("RANK.EQ", "ORDEM.EQ"),
    ("PERCENTILE", "PERCENTIL"),
    ("QUARTILE", "QUARTIL"),
    ("COUNT", "CONT.NÚM"),
    ("COUNTA", "CONT.VALORES"),
    ("COUNTBLANK", "CONTAR.VAZIO"),
    ("SLOPE", "INCLINAÇÃO"),
    ("INTERCEPT", "INTERCEPÇÃO"),
    ("FORECAST", "PREVISÃO"),
    // Text.
    ("CONCATENATE", "CONCATENAR"),
    ("TEXTJOIN", "UNIRTEXTO"),
    ("LEFT", "ESQUERDA"),
    ("RIGHT", "DIREITA"),
    ("MID", "EXT.TEXTO"),
    ("LEN", "NÚM.CARACT"),
    ("LOWER", "MINÚSCULA"),
    ("UPPER", "MAIÚSCULA"),
    ("PROPER", "PRI.MAIÚSCULA"),
    ("TRIM", "ARRUMAR"),
    ("CLEAN", "TIRAR"),
    ("SUBSTITUTE", "SUBSTITUIR"),
    ("REPLACE", "MUDAR"),
    ("FIND", "PROCURAR"),
    ("SEARCH", "LOCALIZAR"),
    ("EXACT", "EXATO"),
    ("CHAR", "CARACT"),
    ("CODE", "CÓDIGO"),
    ("VALUE", "VALOR"),
    ("TEXT", "TEXTO"),
    ("FIXED", "DEF.NÚM.DEC"),
    ("DOLLAR", "MOEDA"),
    // Date and time.
    ("DATE", "DATA"),
    ("TIME", "TEMPO"),
    ("YEAR", "ANO"),
    ("MONTH", "MÊS"),
    ("DAY", "DIA"),
    ("HOUR", "HORA"),
    ("MINUTE", "MINUTO"),
    ("SECOND", "SEGUNDO"),
    ("NOW", "AGORA"),
    ("TODAY", "HOJE"),
    ("WEEKDAY", "DIA.DA.SEMANA"),
    ("WEEKNUM", "NÚMSEMANA"),
    ("ISOWEEKNUM", "NÚMSEMANAISO"),
    ("EDATE", "DATAM"),
    ("EOMONTH", "FIMMÊS"),
    ("DATEDIF", "DATADIF"),
    ("DATEVALUE", "DATA.VALOR"),
    ("DAYS", "DIAS"),
    ("DAYS360", "DIAS360"),
    ("NETWORKDAYS", "DIATRABALHOTOTAL"),
    ("WORKDAY", "DIATRABALHO"),
    ("YEARFRAC", "FRAÇÃOANO"),
    // Lookup and reference.
    ("VLOOKUP", "PROCV"),
    ("HLOOKUP", "PROCH"),
    ("LOOKUP", "PROC"),
    ("XLOOKUP", "PROCX"),
    ("MATCH", "CORRESP"),
    ("INDEX", "ÍNDICE"),
    ("ROW", "LIN"),
    ("COLUMN", "COL"),
    ("OFFSET", "DESLOC"),
    ("INDIRECT", "INDIRETO"),
    ("AGGREGATE", "AGREGAR"),
    ("ISREF", "ÉREF"),
    ("CELL", "CÉL"),
    ("FORMULATEXT", "FÓRMULATEXTO"),
    ("ROWS", "LINS"),
    ("COLUMNS", "COLS"),
    ("TRANSPOSE", "MTRANS"),
    ("ADDRESS", "ENDEREÇO"),
    ("CHOOSE", "ESCOLHER"),
    ("FILTER", "FILTRO"),
    ("SORT", "CLASSIFICAR"),
    ("SORTBY", "CLASSIFICARPOR"),
    ("UNIQUE", "ÚNICO"),
    ("SEQUENCE", "SEQUÊNCIA"),
    // Financial.
    ("PMT", "PGTO"),
    ("IPMT", "IPGTO"),
    ("PPMT", "PPGTO"),
    ("PV", "VP"),
    ("FV", "VF"),
    ("RATE", "TAXA"),
    ("NPV", "VPL"),
    ("IRR", "TIR"),
    ("XNPV", "XVPL"),
    ("XIRR", "XTIR"),
    ("MIRR", "MTIR"),
    ("SLN", "DPD"),
    ("SYD", "SDA"),
    ("DB", "BD"),
    ("DDB", "BDD"),
    ("VDB", "BDV"),
    ("EFFECT", "EFETIVA"),
    ("CUMIPMT", "PGTOJURACUM"),
    ("CUMPRINC", "PGTOCAPACUM"),
    ("PDURATION", "DURAÇÃOP"),
    ("MDURATION", "MDURAÇÃO"),
    ("XMATCH", "CORRESPX"),
    ("TIMEVALUE", "VALOR.TEMPO"),
    ("MINVERSE", "MATRIZ.INVERSO"),
    ("TEXTBEFORE", "TEXTOANTES"),
    ("TEXTAFTER", "TEXTODEPOIS"),
    ("TEXTSPLIT", "DIVIDIRTEXTO"),
    ("VSTACK", "EMPILHARV"),
    ("HSTACK", "EMPILHARH"),
    ("ISFORMULA", "ÉFÓRMULA"),
    // Information and logic.
    ("ISBLANK", "ÉCÉL.VAZIA"),
    ("ISERR", "ÉERRO"),
    ("ISERROR", "ÉERROS"),
    ("ISNA", "É.NÃO.DISP"),
    ("ISNUMBER", "ÉNÚM"),
    ("ISTEXT", "ÉTEXTO"),
    ("ISNONTEXT", "É.NÃO.TEXTO"),
    ("ISLOGICAL", "ÉLÓGICO"),
    ("ISEVEN", "ÉPAR"),
    ("ISODD", "ÉIMPAR"),
    ("TYPE", "TIPO"),
    ("NA", "NÃO.DISP"),
    ("AND", "E"),
    ("OR", "OU"),
    ("XOR", "XOU"),
    ("NOT", "NÃO"),
    ("TRUE", "VERDADEIRO"),
    ("FALSE", "FALSO"),
    ("IF", "SE"),
    ("IFS", "SES"),
    ("IFERROR", "SEERRO"),
    ("IFNA", "SEND"),
    ("SWITCH", "PARÂMETRO"),
    ("HYPERLINK", "HIPERLINK"),
];

impl FormulaLocale {
    /// Function-name table of this syntax; empty for English.
    fn table(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::En => &[],
            Self::Es => SPANISH,
            Self::Pt => PORTUGUESE,
        }
    }
    /// Spellings of the boolean constants: (true, false).
    fn booleans(self) -> (&'static str, &'static str) {
        match self {
            Self::En => ("TRUE", "FALSE"),
            Self::Es => ("VERDADERO", "FALSO"),
            Self::Pt => ("VERDADEIRO", "FALSO"),
        }
    }
    pub(crate) fn boolean_alias(self, name: &str) -> Option<bool> {
        let (yes, no) = self.booleans();
        if self == Self::En {
            None
        } else if crate::same_name(name, yes) {
            Some(true)
        } else if crate::same_name(name, no) {
            Some(false)
        } else {
            None
        }
    }
    pub fn list_separator(self) -> char {
        if self == Self::En { ',' } else { ';' }
    }
    pub fn decimal_separator(self) -> char {
        if self == Self::En { '.' } else { ',' }
    }
    /// Excel pt-BR, like es-ES, writes array columns with `\` and rows with `;`.
    pub fn array_column_separator(self) -> char {
        if self == Self::En { ',' } else { '\\' }
    }
    pub fn function_name(self, canonical: &str) -> &str {
        self.table().iter().find(|(en, _)| en.eq_ignore_ascii_case(canonical)).map(|(_, local)| *local).unwrap_or(canonical)
    }
    /// Names match like the evaluator matches them: case-insensitive, accents exact.
    pub fn canonical_name(self, localized: &str) -> &str {
        self.table().iter().find(|(_, local)| crate::same_name(local, localized)).map(|(en, _)| *en).unwrap_or(localized)
    }
}

/// Parses UI input using the selected function table and separators. Existing names and
/// LET/LAMBDA bindings take precedence over aliases. File and API syntax uses [`crate::parse`].
pub fn parse_input(src: &str, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<Expr, ParseError> {
    let mut expr = parser::parse_syntax(src, locale)?;
    if locale != FormulaLocale::En {
        normalize(&mut expr, locale, &known_name, &mut Bindings::default(), 0, false)?;
    }
    Ok(expr)
}

/// Prints canonical expressions for editing. Name collisions retain the canonical spelling
/// so displaying then committing a formula never changes a defined-name reference.
pub fn print_input(expr: &Expr, locale: FormulaLocale, known_name: impl Fn(&str) -> bool) -> Result<String, ParseError> {
    // Check before cloning/printing. A flat operator chain such as A1+A2+… nests on its left
    // without nesting in the text: cloning, printing and `normalize` follow that left spine in
    // a loop, so it does not count (Excel accepts 4,096 operands, #916). Other nesting does.
    let mut pending = vec![(expr, 0usize)];
    while let Some((node, depth)) = pending.pop() {
        if depth > parser::MAX_DEPTH {
            return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
        }
        match node {
            Expr::Unary(_, child) | Expr::Paren(child) => pending.push((child, depth + 1)),
            Expr::Binary(_, left, right) => {
                pending.push((left, depth));
                pending.push((right, depth + 1));
            }
            Expr::Call(_, args) => pending.extend(args.iter().map(|e| (e, depth + 1))),
            Expr::Invoke(callee, args) => {
                pending.push((callee, depth + 1));
                pending.extend(args.iter().map(|e| (e, depth + 1)));
            }
            Expr::Array(rows) => pending.extend(rows.iter().flatten().map(|e| (e, depth + 1))),
            _ => {}
        }
    }
    if locale == FormulaLocale::En {
        return Ok(crate::print(expr));
    }
    let mut expr = expr.clone();
    normalize(&mut expr, locale, &known_name, &mut Bindings::default(), 0, true)?;
    let canonical = crate::print(&expr);
    let tokens = crate::lexer::tokenize(&canonical).map_err(|e| ParseError { msg: e.msg, pos: e.pos })?;
    let mut out = String::new();
    let mut array_depth = 0usize;
    for token in tokens {
        use crate::lexer::Tok;
        let text = canonical.get(token.start..token.end).unwrap_or("");
        match token.tok {
            Tok::LBrace => {
                array_depth += 1;
                out.push_str(text);
            }
            Tok::RBrace => {
                array_depth = array_depth.saturating_sub(1);
                out.push_str(text);
            }
            Tok::Number(_) => out.push_str(&text.replace('.', &locale.decimal_separator().to_string())),
            Tok::Comma => out.push(if array_depth > 0 { locale.array_column_separator() } else { locale.list_separator() }),
            _ => out.push_str(text),
        }
    }
    Ok(out)
}

/// The LET/LAMBDA names in scope, with how many times each is bound, so a lookup is one probe
/// instead of a scan of every binding for each call.
#[derive(Default)]
struct Bindings {
    stack: Vec<String>,
    counts: std::collections::HashMap<String, usize>,
}

impl Bindings {
    fn len(&self) -> usize {
        self.stack.len()
    }

    fn push(&mut self, name: &str) {
        let key = crate::fold_name(name);
        *self.counts.entry(key.clone()).or_default() += 1;
        self.stack.push(key);
    }

    fn truncate(&mut self, len: usize) {
        while self.stack.len() > len {
            let Some(key) = self.stack.pop() else { break };
            if let Some(n) = self.counts.get_mut(&key) {
                *n -= 1;
                if *n == 0 {
                    self.counts.remove(&key);
                }
            }
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.counts.contains_key(&crate::fold_name(name))
    }
}

fn normalize(expr: &mut Expr, locale: FormulaLocale, known_name: &impl Fn(&str) -> bool, bindings: &mut Bindings, depth: usize, display: bool) -> Result<(), ParseError> {
    if depth > parser::MAX_DEPTH {
        return Err(ParseError { msg: "formula is nested too deeply".into(), pos: 0 });
    }
    let bound = |name: &str, bindings: &Bindings| bindings.contains(name) || known_name(name);
    match expr {
        Expr::Bool(value) if display => {
            let (yes, no) = locale.booleans();
            let alias = if *value { yes } else { no };
            if !bound(alias, bindings) {
                *expr = Expr::Name(alias.into());
            }
        }
        Expr::Name(name) if !display && !bound(name, bindings) => {
            if let Some(value) = locale.boolean_alias(name) {
                *expr = Expr::Bool(value);
            }
        }
        Expr::Call(name, args) if name == "LET" && args.len() >= 3 && !args.len().is_multiple_of(2) => {
            let saved = bindings.len();
            if let Some((body, pairs)) = args.split_last_mut() {
                for pair in pairs.chunks_mut(2) {
                    if let [declaration, value] = pair {
                        // The new name is not in scope in its own value expression.
                        normalize(value, locale, known_name, bindings, depth + 1, display)?;
                        if let Expr::Name(name) = declaration {
                            bindings.push(name);
                        }
                    }
                }
                normalize(body, locale, known_name, bindings, depth + 1, display)?;
            }
            bindings.truncate(saved);
        }
        Expr::Call(name, args) if name == "LAMBDA" => {
            let saved = bindings.len();
            if let Some((body, params)) = args.split_last_mut() {
                for param in params {
                    if let Expr::Name(name) = param {
                        bindings.push(name);
                    }
                }
                normalize(body, locale, known_name, bindings, depth + 1, display)?;
            }
            bindings.truncate(saved);
        }
        Expr::Call(name, args) => {
            let alias = if display { locale.function_name(name) } else { locale.canonical_name(name) };
            if !bound(name, bindings) && (!display || !bound(alias, bindings)) {
                *name = alias.to_string();
            }
            for arg in args {
                normalize(arg, locale, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Invoke(callee, args) => {
            normalize(callee, locale, known_name, bindings, depth + 1, display)?;
            for arg in args {
                normalize(arg, locale, known_name, bindings, depth + 1, display)?;
            }
        }
        Expr::Unary(_, child) | Expr::Paren(child) => normalize(child, locale, known_name, bindings, depth + 1, display)?,
        Expr::Binary(..) => {
            // Follow the left spine of a flat chain (A1+A2+…) in a loop: it is not nesting.
            // Each call leaves `bindings` as it found them, so the order does not matter.
            let mut node = expr;
            while let Expr::Binary(_, left, right) = node {
                normalize(right, locale, known_name, bindings, depth + 1, display)?;
                node = left;
            }
            normalize(node, locale, known_name, bindings, depth, display)?;
        }
        Expr::Array(rows) if display => {
            for row in rows {
                for child in row {
                    normalize(child, locale, known_name, bindings, depth + 1, display)?;
                }
            }
        }
        // Array elements have already been parsed as literals. Sheet/structured references
        // contain user-defined labels and must not be translated.
        _ => {}
    }
    Ok(())
}
