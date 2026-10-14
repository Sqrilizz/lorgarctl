#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

pub fn send_desktop_notification(
    title: &str,
    body: &str,
    urgency: Urgency,
    icon: Option<&str>,
) -> bool {
    let mut cmd = std::process::Command::new("notify-send");
    cmd.arg("-a").arg("LORGAR MSA10W");
    if let Some(i) = icon {
        cmd.arg("-i").arg(i);
    } else {
        cmd.arg("-i").arg("input-mouse");
    }
    match urgency {
        Urgency::Low => {
            cmd.arg("-u").arg("low");
        }
        Urgency::Normal => {
            cmd.arg("-u").arg("normal");
        }
        Urgency::Critical => {
            cmd.arg("-u").arg("critical");
        }
    }
    cmd.arg(title).arg(body);
    cmd.status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_urgency_variants() {
        assert_eq!(Urgency::Low, Urgency::Low);
        assert_ne!(Urgency::Normal, Urgency::Critical);
    }
}
