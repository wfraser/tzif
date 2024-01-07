use std::env;
use std::fs::File;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() -> std::io::Result<()> {
    let rn = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
    let info = tzif::TimeZoneInfo::parse(File::open(env::args_os().nth(1).unwrap())?)?;

    println!("currently: {:?}", info.at(SystemTime::now()));
    println!();
    println!("all transitions:");

    let mut found = false;
    for t in info.iter_transitions() {
        if t.at_time.to_ut(&t.local) > rn && !found {
            println!("--- now ---");
            found = true;
        }
        let hours = t.local.ut_offset_secs as f64 / 60. / 60.;
        println!("at {:?}, {} (UTC{}{}){}",
            t.at_time,
            t.local.desig,
            if hours > 0. { '+' } else { '-' },
            hours.abs(),
            if t.local.is_dst { " (DST)" } else { "" },
        );
    }

    Ok(())
}
