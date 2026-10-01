//! CAMT.053 (ISO 20022 bank-to-customer statement) parser.
//!
//! Event-based with a stack of local element names, so namespace prefixes and the
//! differences between camt.053.001.02 … .08 (e.g. `Cdtr/Nm` vs `Cdtr/Pty/Nm`,
//! `Sts` vs `Sts/Cd`) don't matter. One transaction per `Ntry`; batch entries with
//! several `TxDtls` keep the entry amount and join the details into the description.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Statement {
    pub iban: Option<String>,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub date: String,
    pub amount_cents: i64,
    pub description: String,
    /// Stable reference for duplicate detection.
    pub reference: String,
    /// The creditor's IBAN for a debit, the debtor's for a credit.
    pub counterparty_iban: Option<String>,
}

#[derive(Default)]
struct RawEntry {
    amount: Option<String>,
    credit: Option<bool>,
    booking_date: Option<String>,
    value_date: Option<String>,
    status: Option<String>,
    acct_svcr_ref: Option<String>,
    tx_ref: Option<String>,
    end_to_end: Option<String>,
    additional_info: Option<String>,
    creditor: Vec<String>,
    debtor: Vec<String>,
    creditor_iban: Option<String>,
    debtor_iban: Option<String>,
    remittance: Vec<String>,
}

#[cfg(test)]
pub fn parse(xml: &str) -> Result<Vec<Statement>, String> {
    parse_with_progress(xml, |_| {})
}

/// Parses a statement, reporting the fraction of input consumed (0.0–1.0) as it goes.
pub fn parse_with_progress(xml: &str, mut progress: impl FnMut(f64)) -> Result<Vec<Statement>, String> {
    let total = xml.len().max(1) as f64;
    let mut last_reported = 0.0;
    // No trim_text: it would also trim the text pieces around entity references
    // ("A &amp; B" → "A&B"). Values are trimmed per element instead.
    let mut reader = Reader::from_str(xml);

    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut statements: Vec<Statement> = Vec::new();
    let mut raw: Vec<RawEntry> = Vec::new();
    let mut entry: Option<RawEntry> = None;
    let mut saw_document = false;

    loop {
        let ev = reader
            .read_event()
            .map_err(|e| format!("Invalid XML at position {}: {e}", reader.error_position()))?;
        match ev {
            Event::Start(e) => {
                let name = e.local_name().as_ref().to_string();
                match name.as_str() {
                    "BkToCstmrStmt" => saw_document = true,
                    "Stmt" => {
                        statements.push(Statement { iban: None, entries: Vec::new() });
                        raw.clear();
                    }
                    "Ntry" => entry = Some(RawEntry::default()),
                    _ => {}
                }
                path.push(name);
                text.clear();
            }
            Event::Empty(_) => {}
            Event::Text(e) => {
                text.push_str(&e.xml10_content());
            }
            Event::CData(e) => {
                text.push_str(&e.xml10_content());
            }
            Event::GeneralRef(e) => {
                if let Some(ch) = e.resolve_char_ref().map_err(|e| e.to_string())? {
                    text.push(ch);
                } else {
                    let name = e.xml10_content();
                    let resolved = resolve_predefined_entity(&name)
                        .ok_or_else(|| format!("Unknown entity &{name};"))?;
                    text.push_str(resolved);
                }
            }
            Event::End(_) => {
                let value = text.trim().to_string();
                text.clear();
                let name = path.last().cloned().unwrap_or_default();
                if let Some(pos) = path.iter().rposition(|p| p == "Ntry") {
                    if let Some(en) = entry.as_mut() {
                        if !value.is_empty() {
                            collect(en, &path[pos + 1..], value);
                        }
                    }
                } else if path.ends_with(&["Stmt".into(), "Acct".into(), "Id".into(), "IBAN".into()]) {
                    if let Some(s) = statements.last_mut() {
                        s.iban = Some(value.replace(' ', "").to_uppercase());
                    }
                }
                if name == "Ntry" {
                    if let Some(en) = entry.take() {
                        raw.push(en);
                    }
                    let done = reader.buffer_position() as f64 / total;
                    if done - last_reported >= 0.01 {
                        last_reported = done;
                        progress(done);
                    }
                }
                if name == "Stmt" {
                    let stmt = statements.last_mut().expect("Stmt start pushed a statement");
                    stmt.entries = finish_entries(std::mem::take(&mut raw))?;
                }
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !saw_document {
        return Err("Not a CAMT.053 file (BkToCstmrStmt is missing)".into());
    }
    progress(1.0);
    Ok(statements)
}

fn collect(en: &mut RawEntry, rel: &[String], value: String) {
    let rel: Vec<&str> = rel.iter().map(String::as_str).collect();
    match rel.as_slice() {
        ["Amt"] => en.amount = Some(value),
        ["CdtDbtInd"] => en.credit = Some(value == "CRDT"),
        ["Sts"] | ["Sts", "Cd"] => en.status = Some(value),
        ["BookgDt", "Dt"] | ["BookgDt", "DtTm"] => en.booking_date = Some(value),
        ["ValDt", "Dt"] | ["ValDt", "DtTm"] => en.value_date = Some(value),
        ["AcctSvcrRef"] => en.acct_svcr_ref = Some(value),
        ["AddtlNtryInf"] => en.additional_info = Some(value),
        ["NtryDtls", "TxDtls", rest @ ..] => match rest {
            ["Refs", "AcctSvcrRef"] => {
                en.tx_ref.get_or_insert(value);
            }
            ["Refs", "EndToEndId"] => {
                en.end_to_end.get_or_insert(value);
            }
            ["RmtInf", "Ustrd"] => en.remittance.push(value),
            ["RltdPties", "Cdtr", "Nm"] | ["RltdPties", "Cdtr", "Pty", "Nm"] => en.creditor.push(value),
            ["RltdPties", "Dbtr", "Nm"] | ["RltdPties", "Dbtr", "Pty", "Nm"] => en.debtor.push(value),
            ["RltdPties", "CdtrAcct", "Id", "IBAN"] => {
                en.creditor_iban.get_or_insert(value);
            }
            ["RltdPties", "DbtrAcct", "Id", "IBAN"] => {
                en.debtor_iban.get_or_insert(value);
            }
            _ => {}
        },
        _ => {}
    }
}

fn finish_entries(raw: Vec<RawEntry>) -> Result<Vec<Entry>, String> {
    let mut out: Vec<Entry> = Vec::new();
    for en in raw {
        // Only booked entries; pending/info entries may still change or disappear.
        if let Some(st) = &en.status {
            if st != "BOOK" {
                continue;
            }
        }
        let amount = en.amount.as_deref().ok_or("Ntry without Amt")?;
        let mut cents = parse_decimal(amount).ok_or_else(|| format!("Invalid amount: {amount}"))?;
        match en.credit {
            Some(true) => {}
            Some(false) => cents = -cents,
            None => return Err("Ntry without CdtDbtInd".into()),
        }
        let date = en
            .booking_date
            .or(en.value_date)
            .ok_or("Ntry without booking date")?;
        let date: String = date.chars().take(10).collect();
        if !is_iso_date(&date) {
            return Err(format!("Invalid date: {date}"));
        }

        let counterparty = if cents < 0 { &en.creditor } else { &en.debtor };
        let mut parts: Vec<String> = Vec::new();
        if !counterparty.is_empty() {
            parts.push(dedup_join(counterparty));
        }
        if !en.remittance.is_empty() {
            parts.push(dedup_join(&en.remittance));
        }
        let description = if !parts.is_empty() {
            parts.join(" – ")
        } else {
            en.additional_info.unwrap_or_else(|| "CAMT-import".into())
        };

        let base_ref = en
            .acct_svcr_ref
            .or(en.tx_ref)
            .or(en.end_to_end.filter(|r| r != "NOTPROVIDED"))
            .unwrap_or_else(|| format!("{date}|{cents}|{description}"));
        // Some banks reuse references; make repeats within one statement unique and stable.
        let n = out.iter().filter(|e| e.reference == base_ref || e.reference.starts_with(&format!("{base_ref}#"))).count();
        let reference = if n == 0 { base_ref } else { format!("{base_ref}#{n}") };

        let counterparty_iban = if cents < 0 { en.creditor_iban } else { en.debtor_iban };
        let counterparty_iban = counterparty_iban.and_then(|i| fin_shared::normalize_iban(&i));
        out.push(Entry { date, amount_cents: cents, description, reference, counterparty_iban });
    }
    Ok(out)
}

fn dedup_join(items: &[String]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for i in items {
        if !seen.contains(&i.as_str()) {
            seen.push(i);
        }
    }
    seen.join(" ")
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// ISO 20022 decimal (`1234.5`) to cents. Rejects sub-cent precision.
fn parse_decimal(s: &str) -> Option<i64> {
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if int.is_empty() || !int.bytes().all(|c| c.is_ascii_digit()) || !frac.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let frac = frac.trim_end_matches('0');
    if frac.len() > 2 {
        return None;
    }
    let cents: i64 = format!("{frac:0<2}").parse().ok()?;
    int.parse::<i64>().ok()?.checked_mul(100)?.checked_add(cents)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const SAMPLE_V02: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
  <BkToCstmrStmt>
    <GrpHdr><MsgId>1</MsgId></GrpHdr>
    <Stmt>
      <Id>S1</Id>
      <Acct><Id><IBAN>NL91 ABNA 0417 1643 00</IBAN></Id></Acct>
      <Bal><Amt Ccy="EUR">10.00</Amt></Bal>
      <Ntry>
        <Amt Ccy="EUR">42.5</Amt>
        <CdtDbtInd>DBIT</CdtDbtInd>
        <Sts>BOOK</Sts>
        <BookgDt><Dt>2026-09-02</Dt></BookgDt>
        <ValDt><Dt>2026-09-03</Dt></ValDt>
        <AcctSvcrRef>REF-1</AcctSvcrRef>
        <NtryDtls><TxDtls>
          <RltdPties>
            <Dbtr><Nm>J Jansen</Nm></Dbtr>
            <DbtrAcct><Id><IBAN>NL91ABNA0417164300</IBAN></Id></DbtrAcct>
            <Cdtr><Nm>Albert Heijn &amp; Zn</Nm></Cdtr>
            <CdtrAcct><Id><IBAN>NL44 INGB 0000 1234 56</IBAN></Id></CdtrAcct>
          </RltdPties>
          <RmtInf><Ustrd>Boodschappen week 36</Ustrd></RmtInf>
        </TxDtls></NtryDtls>
      </Ntry>
      <Ntry>
        <Amt Ccy="EUR">3000.00</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <Sts>BOOK</Sts>
        <BookgDt><Dt>2026-09-25</Dt></BookgDt>
        <AcctSvcrRef>REF-2</AcctSvcrRef>
        <NtryDtls><TxDtls>
          <RltdPties><Dbtr><Nm>Werkgever BV</Nm></Dbtr><DbtrAcct><Id><IBAN>NL20INGB0001234567</IBAN></Id></DbtrAcct></RltdPties>
          <RmtInf><Ustrd>Salaris september</Ustrd></RmtInf>
        </TxDtls></NtryDtls>
      </Ntry>
      <Ntry>
        <Amt Ccy="EUR">1.00</Amt>
        <CdtDbtInd>DBIT</CdtDbtInd>
        <Sts>PDNG</Sts>
        <BookgDt><Dt>2026-09-29</Dt></BookgDt>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;

    const SAMPLE_V08_PREFIXED: &str = r#"<?xml version="1.0"?>
<c:Document xmlns:c="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
  <c:BkToCstmrStmt><c:Stmt>
    <c:Acct><c:Id><c:IBAN>NL02RABO0123456789</c:IBAN></c:Id></c:Acct>
    <c:Ntry>
      <c:Amt Ccy="EUR">12.30</c:Amt>
      <c:CdtDbtInd>DBIT</c:CdtDbtInd>
      <c:Sts><c:Cd>BOOK</c:Cd></c:Sts>
      <c:BookgDt><c:DtTm>2026-08-31T23:10:00+02:00</c:DtTm></c:BookgDt>
      <c:NtryDtls><c:TxDtls>
        <c:Refs><c:EndToEndId>NOTPROVIDED</c:EndToEndId></c:Refs>
        <c:RltdPties><c:Cdtr><c:Pty><c:Nm>NS Reizigers</c:Nm></c:Pty></c:Cdtr><c:CdtrAcct><c:Id><c:IBAN>NL27INGB0000026500</c:IBAN></c:Id></c:CdtrAcct></c:RltdPties>
      </c:TxDtls></c:NtryDtls>
      <c:AddtlNtryInf>Betaalautomaat</c:AddtlNtryInf>
    </c:Ntry>
    <c:Ntry>
      <c:Amt Ccy="EUR">12.30</c:Amt>
      <c:CdtDbtInd>DBIT</c:CdtDbtInd>
      <c:Sts><c:Cd>BOOK</c:Cd></c:Sts>
      <c:BookgDt><c:DtTm>2026-08-31T23:10:00+02:00</c:DtTm></c:BookgDt>
      <c:NtryDtls><c:TxDtls>
        <c:RltdPties><c:Cdtr><c:Pty><c:Nm>NS Reizigers</c:Nm></c:Pty></c:Cdtr></c:RltdPties>
      </c:TxDtls></c:NtryDtls>
    </c:Ntry>
  </c:Stmt></c:BkToCstmrStmt>
</c:Document>"#;

    #[test]
    fn parses_v02_with_entities_and_skips_pending() {
        let st = parse(SAMPLE_V02).unwrap();
        assert_eq!(st.len(), 1);
        assert_eq!(st[0].iban.as_deref(), Some("NL91ABNA0417164300"));
        assert_eq!(
            st[0].entries,
            vec![
                Entry {
                    date: "2026-09-02".into(),
                    amount_cents: -4250,
                    description: "Albert Heijn & Zn – Boodschappen week 36".into(),
                    reference: "REF-1".into(),
                    // Debit: the creditor is the counterparty, not our own debtor account.
                    counterparty_iban: Some("NL44INGB0000123456".into()),
                },
                Entry {
                    date: "2026-09-25".into(),
                    amount_cents: 300000,
                    description: "Werkgever BV – Salaris september".into(),
                    reference: "REF-2".into(),
                    counterparty_iban: Some("NL20INGB0001234567".into()),
                },
            ]
        );
    }

    #[test]
    fn parses_v08_prefixed_and_disambiguates_references() {
        let st = parse(SAMPLE_V08_PREFIXED).unwrap();
        let e = &st[0].entries;
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].date, "2026-08-31");
        assert_eq!(e[0].amount_cents, -1230);
        assert_eq!(e[0].description, "NS Reizigers");
        assert_eq!(e[0].counterparty_iban.as_deref(), Some("NL27INGB0000026500"));
        assert_eq!(e[1].counterparty_iban, None);
        assert_ne!(e[0].reference, e[1].reference);
    }

    #[test]
    fn rejects_non_camt_and_bad_amounts() {
        assert!(parse("<Document><Foo/></Document>").is_err());
        assert!(parse("not xml <<<").is_err());
        assert_eq!(parse_decimal("1.005"), None);
        assert_eq!(parse_decimal("1.50"), Some(150));
        assert_eq!(parse_decimal("7"), Some(700));
    }
}

#[cfg(test)]
pub use tests::SAMPLE_V02;
