use crate::{Capture, Token};

#[derive(Clone, Copy)]
pub struct Model {
    pub rdna: bool,
}
impl Model {
    pub fn from_capture(capture: &Capture) -> Option<Self> {
        let arch = capture.architecture.trim().to_ascii_lowercase();
        let gfx = arch.strip_prefix("gfx");
        if arch == "navi"
            || gfx.is_some_and(|s| {
                s.len() == 4
                    && s.is_ascii()
                    && matches!(&s[..2], "10" | "11" | "12")
                    && s.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Some(Self { rdna: true });
        }
        if arch == "vega"
            || gfx.is_some_and(|s| {
                s.len() == 3
                    && matches!(s.as_bytes()[0], b'6'..=b'9')
                    && s.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            return Some(Self { rdna: false });
        }
        match capture
            .metadata
            .get("gfxip")
            .and_then(|v| v.parse::<f64>().ok())
        {
            Some(v) if v.fract() == 0. && (6. ..=12.).contains(&v) => Some(Self { rdna: v >= 10. }),
            _ => None,
        }
    }
    pub fn pipe(self, category: u32, separate_lds: bool) -> (&'static str, bool) {
        match category {
            9 | 10 => ("IMMED", true),
            11 => ("MSG", true),
            13 if self.rdna => ("MATRIX", false),
            6 | 13 => ("VALU", false),
            5 if self.rdna || separate_lds => ("LDS", false),
            12 if self.rdna => ("RAY", false),
            3 | 4 | 5 | 12 => ("VMEM", false),
            7 | 8 if self.rdna => ("JUMP", true),
            _ => ("SCAL", false),
        }
    }
    pub fn project(self, token: &mut Token, isa: &str) -> Result<bool, String> {
        let isa = isa.trim().to_ascii_lowercase();
        if ["v_mfma", "v_smfma", "v_wmma", "v_swmma"]
            .iter()
            .any(|prefix| isa.starts_with(prefix))
        {
            token.category = 13;
        }
        if token.category > 13 {
            return Ok(false);
        }
        let cycles = token.duration.max(token.stall);
        let base = if self.rdna { 1. } else { 4. };
        let stall = if matches!(token.category, 9..=11) {
            (cycles - base).max(0.)
        } else {
            (cycles - base).min(token.stall).max(0.)
        };
        let end = token.start + cycles;
        token.start += stall;
        token.duration = end - token.start;
        token.stall = 0.;
        if !token.start.is_finite() || !end.is_finite() {
            return Err("Non-finite utilization extent".into());
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn architecture_validation_and_unknown_categories() {
        let mut capture = Capture::empty("test".into(), "test", "cycles");
        for arch in ["gfx908", "gfx90a", "gfx942", "gfx950", "vega"] {
            capture.architecture = arch.into();
            assert!(!Model::from_capture(&capture).unwrap().rdna);
        }
        for arch in [
            "gfx1010", "gfx1030", "gfx1100", "gfx1151", "gfx1200", "navi",
        ] {
            capture.architecture = arch.into();
            assert!(Model::from_capture(&capture).unwrap().rdna);
        }
        for arch in ["unknown", "gfx1300", "gfx942invalid", "gfx🦀"] {
            capture.architecture = arch.into();
            assert!(Model::from_capture(&capture).is_none());
        }
        let mut token = Token {
            start: 0.,
            duration: 20.,
            stall: 0.,
            category: 99,
            instruction: 1,
        };
        assert!(!Model { rdna: true }.project(&mut token, "").unwrap());
    }
    #[test]
    fn issue_quantum_and_pipe_routing_match_the_detailed_view() {
        let gcn = [
            "SCAL", "SCAL", "SCAL", "VMEM", "VMEM", "VMEM", "VALU", "SCAL", "SCAL", "IMMED",
            "IMMED", "MSG", "VMEM", "VALU",
        ];
        let rdna = [
            "SCAL", "SCAL", "SCAL", "VMEM", "VMEM", "LDS", "VALU", "JUMP", "JUMP", "IMMED",
            "IMMED", "MSG", "RAY", "MATRIX",
        ];
        for (is_rdna, pipes) in [(false, gcn), (true, rdna)] {
            let model = Model { rdna: is_rdna };
            for category in 0..14 {
                assert_eq!(model.pipe(category, false).0, pipes[category as usize]);
                assert_eq!(
                    model.pipe(category, false).1,
                    matches!(category, 9..=11) || is_rdna && matches!(category, 7 | 8)
                );
                for (duration, stall) in [(20., 20.), (2., 2.), (0., 0.), (2., 20.), (20., 0.)] {
                    let mut token = Token {
                        start: 100.,
                        duration,
                        stall,
                        category,
                        instruction: 1,
                    };
                    assert!(model.project(&mut token, "").unwrap());
                    let cycles: f64 = duration.max(stall);
                    let base = if is_rdna { 1. } else { 4. };
                    let removed = if matches!(category, 9..=11) {
                        (cycles - base).max(0.)
                    } else {
                        (cycles - base).min(stall).max(0.)
                    };
                    assert_eq!(token.start, 100. + removed);
                    assert_eq!(token.start + token.duration, 100. + cycles);
                }
            }
            for isa in ["v_mfma_f32", " V_WMMA_F32", "v_smfma_f32", "v_swmma_f32"] {
                let mut token = Token {
                    start: 0.,
                    duration: 20.,
                    stall: 0.,
                    category: 6,
                    instruction: 1,
                };
                model.project(&mut token, isa).unwrap();
                assert_eq!(token.category, 13);
            }
            assert_eq!(model.pipe(5, true).0, "LDS");
        }
    }
}
