use gridcraft_formula::{FormulaLocale, parse, parse_input, print, print_input};

const PT: FormulaLocale = FormulaLocale::Pt;

fn input(src: &str) -> String {
    print(&parse_input(src, PT, |_| false).unwrap())
}

#[test]
fn portuguese_aliases_and_argument_separators() {
    for (src, expected) in [
        ("=SOMA(A1;A2)", "SUM(A1,A2)"),
        ("=soma(A1:A10)", "SUM(A1:A10)"),
        ("=SE(A1>1,5;\"sim\";\"não\")", "IF(A1>1.5,\"sim\",\"não\")"),
        ("MÉDIA(A1:A10)", "AVERAGE(A1:A10)"),
        ("média(A1:A10)", "AVERAGE(A1:A10)"),
        ("CONT.NÚM(A1:A10)", "COUNT(A1:A10)"),
        ("CONT.VALORES(A1:A10)", "COUNTA(A1:A10)"),
        ("CONT.SE(A1:A10;\">0\")", "COUNTIF(A1:A10,\">0\")"),
        ("CONT.SES(A1:A10;\">0\";B1:B10;\"x\")", "COUNTIFS(A1:A10,\">0\",B1:B10,\"x\")"),
        ("SOMASE(A1:A10;\">0\")", "SUMIF(A1:A10,\">0\")"),
        ("SOMASES(C1:C9;A1:A9;\">0\")", "SUMIFS(C1:C9,A1:A9,\">0\")"),
        ("MÉDIASE(A1:A10;\">0\")", "AVERAGEIF(A1:A10,\">0\")"),
        ("PROCV(A2;D:E;2;FALSO)", "VLOOKUP(A2,D:E,2,FALSE)"),
        ("PROCH(A2;D1:H2;2;FALSO)", "HLOOKUP(A2,D1:H2,2,FALSE)"),
        ("PROCX(A2;D:D;E:E)", "XLOOKUP(A2,D:D,E:E)"),
        ("ÍNDICE(A1:B2;1;2)", "INDEX(A1:B2,1,2)"),
        ("CORRESP(3;A1:A9;0)", "MATCH(3,A1:A9,0)"),
        ("SEERRO(A1/0;0)", "IFERROR(A1/0,0)"),
        ("E(VERDADEIRO;FALSO)", "AND(TRUE,FALSE)"),
        ("OU(A1>1;A2>1)", "OR(A1>1,A2>1)"),
        ("NÃO(A1)", "NOT(A1)"),
        ("MÁXIMO(A1:A3)+MÍNIMO(A1:A3)", "MAX(A1:A3)+MIN(A1:A3)"),
        ("ARRED(A1;2)", "ROUND(A1,2)"),
        ("ARREDONDAR.PARA.CIMA(A1;0)", "ROUNDUP(A1,0)"),
        ("ARREDONDAR.PARA.BAIXO(A1;0)", "ROUNDDOWN(A1,0)"),
        ("HOJE()", "TODAY()"),
        ("AGORA()", "NOW()"),
        ("DATA(2027;3;18)", "DATE(2027,3,18)"),
        ("ANO(A1)+MÊS(A1)+DIA(A1)", "YEAR(A1)+MONTH(A1)+DAY(A1)"),
        ("TEXTO(A1;\"0,00\")", "TEXT(A1,\"0,00\")"),
        ("CONCATENAR(A1;\" \";B1)", "CONCATENATE(A1,\" \",B1)"),
        ("ESQUERDA(A1;2)&DIREITA(A1;2)&EXT.TEXTO(A1;2;3)", "LEFT(A1,2)&RIGHT(A1,2)&MID(A1,2,3)"),
        ("NÚM.CARACT(ARRUMAR(A1))", "LEN(TRIM(A1))"),
        ("MAIÚSCULA(A1)&MINÚSCULA(A2)", "UPPER(A1)&LOWER(A2)"),
        ("VPL(0,1;A1:A5)", "NPV(0.1,A1:A5)"),
        ("TIR(A1:A5;0,1)", "IRR(A1:A5,0.1)"),
        ("PGTO(0,08/12;10;10000)", "PMT(0.08/12,10,10000)"),
        ("VP(0,05;12;100)", "PV(0.05,12,100)"),
        ("VF(0,05;12;100)", "FV(0.05,12,100)"),
        ("=verdadeiro", "TRUE"),
        ("=falso", "FALSE"),
        ("VERDADEIRO()", "TRUE()"),
        ("LAMBDA(x;x+1)(2)", "LAMBDA(x,x+1)(2)"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
}

#[test]
fn portuguese_names_are_accent_exact_and_english_still_works() {
    // Excel pt-BR does not accept the unaccented spelling: it stays an unknown name.
    assert_eq!(input("MEDIA(A1:A3)"), "MEDIA(A1:A3)");
    assert_eq!(input("INDICE(A1:A3;1)"), "INDICE(A1:A3,1)");
    assert_eq!(input("MÊS(A1)"), "MONTH(A1)");
    assert_eq!(input("MES(A1)"), "MES(A1)");
    // English and Spanish spellings fall through to the engine, as with Spanish input.
    assert_eq!(input("SUM(A1;A2)"), "SUM(A1,A2)");
    assert_eq!(input("SUMA(A1;A2)"), "SUMA(A1,A2)");
}

#[test]
fn portuguese_display_round_trips_to_portuguese() {
    for src in [
        "SOMA(A1;A2)",
        "SE(A1>1,5;\"sim\";\"não\")",
        "SE(VERDADEIRO;SOMA(1,5;2,25);0)",
        "SEERRO(PROCV(A2;D:E;2;FALSO);\"n/d\")",
        "CONT.NÚM(A1:A3)+MÉDIA(B1:B3)",
        r"SOMA({1,5\2;3\4})",
        "SE(A1=\"a,b;c\";CONCATENAR(\"1.5\";\",\";\";\");0)",
    ] {
        let canonical = parse_input(src, PT, |_| false).unwrap();
        let shown = print_input(&canonical, PT, |_| false).unwrap();
        assert_eq!(shown, src);
        assert_eq!(print(&parse_input(&shown, PT, |_| false).unwrap()), print(&canonical));
    }
    // Stored form stays English with `.` decimals and `,` separators.
    let expr = parse("IF(A1>1.5,SUM(A1,A2),NOT(FALSE))").unwrap();
    assert_eq!(print_input(&expr, PT, |_| false).unwrap(), "SE(A1>1,5;SOMA(A1;A2);NÃO(FALSO))");
    assert_eq!(print_input(&expr, FormulaLocale::En, |_| false).unwrap(), print(&expr));
}

#[test]
fn portuguese_input_stores_canonical_english() {
    let expr = parse_input("SE(A1>1,5;\"sim\";\"não\")", PT, |_| false).unwrap();
    assert_eq!(print(&expr), "IF(A1>1.5,\"sim\",\"não\")");
    assert!(parse_input("SUM(1.5;2)", PT, |_| false).is_err());
}

#[test]
fn portuguese_input_preserves_defined_and_local_names() {
    let known = |name: &str| ["SOMA", "FALSO", "média"].iter().any(|n| gridcraft_formula::same_name(n, name));
    let expr = parse_input("SOMA(2;3)+FALSO+SE(VERDADEIRO;4;5)+MÉDIA(1)", PT, known).unwrap();
    assert_eq!(print(&expr), "SOMA(2,3)+FALSO+IF(TRUE,4,5)+MÉDIA(1)");
    for (src, expected) in [
        ("LET(FALSO;7;FALSO)", "LET(FALSO,7,FALSO)"),
        ("LET(falso;FALSO;SE(falso;1;2))", "LET(falso,FALSE,IF(falso,1,2))"),
        ("LET(SOMA;LAMBDA(x;x+1);SOMA(2))", "LET(SOMA,LAMBDA(x,x+1),SOMA(2))"),
        ("LAMBDA(SOMA;SOMA(2))(LAMBDA(x;x+1))", "LAMBDA(SOMA,SOMA(2))(LAMBDA(x,x+1))"),
    ] {
        assert_eq!(input(src), expected, "{src}");
    }
    // Displaying a canonical formula whose name collides keeps the canonical spelling.
    let expr = parse("SUM(1,2)+IF(TRUE,1,2)").unwrap();
    let shown = print_input(&expr, PT, |n| n == "SOMA").unwrap();
    assert_eq!(shown, "SUM(1;2)+SE(VERDADEIRO;1;2)");
}

#[test]
fn portuguese_array_constants_use_backslash_for_columns() {
    let expr = parse_input(r"SOMA({1,5\2;3\4})", PT, |_| false).unwrap();
    assert_eq!(print(&expr), "SUM({1.5,2;3,4})");
}
