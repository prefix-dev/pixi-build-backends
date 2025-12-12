use minijinja::Environment;
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    /// Any additional args to pass to `zig build`
    pub extra_args: Vec<String>,

    /// The platform that is running the build.
    pub is_bash: bool,
}

impl BuildScriptContext {
    pub fn render(&self) -> String {
        let env = Environment::new();
        let template = env
            .template_from_str(include_str!("build_script.j2"))
            .unwrap();
        template.render(self).unwrap().trim().to_string()
    }
}

#[cfg(test)]
mod test {
    use rstest::*;

    #[rstest]
    fn test_build_script(#[values(true, false)] is_bash: bool) {
        let context = super::BuildScriptContext {
            extra_args: vec![],
            is_bash,
        };
        let script = context.render();

        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| {
            insta::assert_snapshot!(script);
        });
    }

    #[rstest]
    fn test_build_script_with_extra_args(#[values(true, false)] is_bash: bool) {
        let context = super::BuildScriptContext {
            extra_args: vec!["-Doptimize=ReleaseFast".to_string()],
            is_bash,
        };
        let script = context.render();

        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| {
            insta::assert_snapshot!(script);
        });
    }
}
