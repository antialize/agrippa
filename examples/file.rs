use agrippa::fs::File;
use agrippa::runtime::{Priority, Reactor, Result};

use log::info;

extern crate simple_logger;

fn main() -> Result<()> {
    simple_logger::init().unwrap();

    let reactor = Reactor::new(1024 * 1024)?;
    let r2 = reactor.clone();

    Reactor::spawn(&reactor.clone(), Priority::Normal, async move {
        let file = File::open("in").await?;

        let data = file.read_all().await?;
        info!("Read file '{}'", std::str::from_utf8(&data).unwrap());
        file.close().await?;

        let file = File::create("out").await?;
        file.write(&data, 0).await?;
        file.close().await?;

        Ok(())
    });

    r2.run()?;

    Ok(())
}
