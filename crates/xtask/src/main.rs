use std::io::Write;

use anyhow::Context;

fn main() -> anyhow::Result<()> {
    let cmd = std::env::args().nth(1).context("Missing command")?;
    match cmd.as_str() {
        "generate-niche-repr" => {
            let bits = std::env::args()
                .nth(2)
                .context("Missing bits argument")?
                .parse::<u32>()
                .context("Invalid bits argument")?;
            generate_niche_repr(&mut std::io::stdout(), bits)
                .context("Failed to generate niche repr")
        }
        _ => {
            anyhow::bail!("Unknown command: {cmd}");
        }
    }
}

fn generate_niche_repr(w: &mut (impl Write + ?Sized), bits: u32) -> anyhow::Result<()> {
    anyhow::ensure!(
        bits.is_power_of_two() && (2..=64).contains(&bits),
        "bits must be a power of two between 2 and 64"
    );

    let width = bits.ilog2() as usize;
    writeln!(
        w,
        "#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]\nenum Repr {{\n    #[default]",
    )?;

    for i in 0..bits {
        writeln!(w, "    _Ob{i:0width$b},")?;
    }

    writeln!(w, "}}")?;

    Ok(())
}
