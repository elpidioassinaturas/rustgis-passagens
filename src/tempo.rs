//! O tempo em segundos desde 1970 (UTC), como `f64`. O plugin não tem relógio: o "agora" vem da
//! resposta do Open-Meteo (ver `comandos.rs`).

/// Dias desde 1970 da data civil (algoritmo `days_from_civil` de Howard Hinnant).
fn dias_desde_1970(ano: i64, mes: u32, dia: u32) -> i64 {
    let a = if mes <= 2 { ano - 1 } else { ano };
    let era = a.div_euclid(400);
    let yoe = a - era * 400;
    let m = i64::from(mes);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(dia) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A data civil de um dia desde 1970.
fn civil(dias: i64) -> (i64, u32, u32) {
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(mes <= 2), mes, dia)
}

/// Lê "AAAA-MM-DD", "AAAA-MM-DDTHH:MM", "AAAA-MM-DDTHH:MM:SS(.fff)(Z)". Sem fuso é UTC.
pub fn ler(texto: &str) -> Option<f64> {
    let t = texto.trim().trim_end_matches('Z');
    let (data, hora) = t.split_once(['T', ' ']).unwrap_or((t, "00:00"));
    let mut d = data.split('-');
    let (ano, mes, dia) = (d.next()?.parse::<i64>().ok()?, d.next()?.parse::<u32>().ok()?, d.next()?.parse::<u32>().ok()?);
    if !(1..=12).contains(&mes) || !(1..=31).contains(&dia) {
        return None;
    }
    let mut h = hora.split(':');
    let horas = h.next()?.parse::<f64>().ok()?;
    let minutos = h.next().unwrap_or("0").parse::<f64>().ok()?;
    let segundos = h.next().unwrap_or("0").trim_end_matches(['+', '-']).parse::<f64>().ok()?;
    Some(dias_desde_1970(ano, mes, dia) as f64 * 86_400.0 + horas * 3600.0 + minutos * 60.0 + segundos)
}

/// "AAAA-MM-DDTHH:MM:SSZ".
pub fn iso(t: f64) -> String {
    let s = t.round() as i64;
    let (ano, mes, dia) = civil(s.div_euclid(86_400));
    let r = s.rem_euclid(86_400);
    format!("{ano:04}-{mes:02}-{dia:02}T{:02}:{:02}:{:02}Z", r / 3600, r % 3600 / 60, r % 60)
}

/// "AAAA-MM-DDTHH:MM:SS.mmm", com os milissegundos (a época dos elementos orbitais).
pub fn iso_ms(t: f64) -> String {
    let ms = (t * 1000.0).round() as i64;
    let s = ms.div_euclid(1000);
    let (ano, mes, dia) = civil(s.div_euclid(86_400));
    let r = s.rem_euclid(86_400);
    format!("{ano:04}-{mes:02}-{dia:02}T{:02}:{:02}:{:02}.{:03}", r / 3600, r % 3600 / 60, r % 60, ms.rem_euclid(1000))
}

/// "AAAA-MM-DD HH:MM", na hora local de um fuso `deslocamento` segundos à frente do UTC.
pub fn local(t: f64, deslocamento: f64) -> String {
    let s = (t + deslocamento).round() as i64;
    let (ano, mes, dia) = civil(s.div_euclid(86_400));
    let r = s.rem_euclid(86_400);
    format!("{ano:04}-{mes:02}-{dia:02} {:02}:{:02}", r / 3600, r % 3600 / 60)
}

/// "qui 19/11 10:02", na hora local.
pub fn curto(t: f64, deslocamento: f64) -> String {
    const SEMANA: [&str; 7] = ["dom", "seg", "ter", "qua", "qui", "sex", "sáb"];
    let s = (t + deslocamento).round() as i64;
    let d = s.div_euclid(86_400);
    let (_, mes, dia) = civil(d);
    let r = s.rem_euclid(86_400);
    // 1970-01-01 foi uma quinta.
    format!("{} {dia:02}/{mes:02} {:02}:{:02}", SEMANA[(d + 4).rem_euclid(7) as usize], r / 3600, r % 3600 / 60)
}

/// "AAAA-MM-DD".
pub fn dia(t: f64) -> String {
    iso(t)[..10].to_string()
}

/// O dia juliano.
pub fn juliano(t: f64) -> f64 {
    t / 86_400.0 + 2_440_587.5
}

/// O tempo sideral médio de Greenwich, em radianos (expressão da IAU).
pub fn gmst(t: f64) -> f64 {
    sgp4::iau_epoch_to_sidereal_time((juliano(t) - 2_451_545.0) / 365.25)
}

/// Uma idade em segundos, em palavras: "40 minutos atrás", "5 horas atrás", "2,5 dias atrás".
pub fn idade(segundos: f64) -> String {
    let s = segundos.max(0.0);
    if s < 2.0 * 3600.0 {
        format!("{:.0} minutos atrás", s / 60.0)
    } else if s < 2.0 * 86_400.0 {
        format!("{:.0} horas atrás", s / 3600.0)
    } else {
        format!("{:.1} dias atrás", s / 86_400.0).replace('.', ",")
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn ida_e_volta() {
        let t = ler("2026-11-15T13:02:49.024000Z").unwrap();
        assert_eq!(iso(t), "2026-11-15T13:02:49Z");
        assert_eq!(ler("1970-01-01").unwrap(), 0.0);
        assert_eq!(ler("2000-03-01T00:00").unwrap(), 951_868_800.0);
        assert_eq!(local(t, -10_800.0), "2026-11-15 10:02");
        assert_eq!(dia(t), "2026-11-15");
        assert_eq!(iso_ms(t), "2026-11-15T13:02:49.024");
        assert!(ler("2026-13-01").is_none() && ler("ontem").is_none());
    }

    #[test]
    fn tempo_sideral() {
        // 2000-01-01T12:00Z: GMST = 280.46°.
        let g = gmst(946_728_000.0).to_degrees();
        assert!((g - 280.46).abs() < 0.01, "{g}");
    }

    #[test]
    fn idade_em_palavras() {
        assert_eq!(idade(40.0 * 60.0), "40 minutos atrás");
        assert_eq!(idade(5.0 * 3600.0), "5 horas atrás");
        assert_eq!(idade(2.5 * 86_400.0), "2,5 dias atrás");
        assert_eq!(idade(-10.0), "0 minutos atrás");
    }
}
