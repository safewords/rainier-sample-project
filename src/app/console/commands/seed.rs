//! `app:seed` — an example of your own command.

use rainier_framework::console_kernel::{exit, Arguments, Command};
use rainier_framework::database::Repository;
use rainier_framework::prelude::*;

use crate::app::repositories::PostRepository;
use crate::database::seeders;

/// Populates the database with demo data.
#[derive(Debug, Default)]
pub struct SeedCommand;

#[async_trait]
impl Command for SeedCommand {
    fn name(&self) -> &str {
        "app:seed"
    }

    fn description(&self) -> &str {
        "Seed the database with demo data"
    }

    fn help(&self) -> Option<&str> {
        Some(
            "Usage:\n  app:seed [--fresh]\n\n\
             Options:\n  --fresh  Delete existing posts first\n\n\
             Safe to run repeatedly: it checks before inserting.",
        )
    }

    async fn handle(&self, args: &Arguments, app: &Application) -> Result<i32> {
        if args.flag("fresh") {
            let posts = app.resolve::<PostRepository>()?;
            let removed = posts.delete_matching(Criteria::new()).await?;
            println!("Deleted {removed} post(s).");
        }

        seeders::seed(app).await?;

        let posts = app.resolve::<PostRepository>()?;
        println!("Seeded. {} post(s) in the database.", posts.count().await?);
        println!("Log in as ada@example.com with the password `correct-horse`.");

        Ok(exit::SUCCESS)
    }
}
