//! `routes/console.php` — the console's command list.

use rainier_framework::console_kernel::Console;

use crate::app::console::commands::SeedCommand;

/// Every command this application answers to.
///
/// `rainier_framework::console` supplies the built-ins — `serve`, `route:list`,
/// `migrate`, `queue:work` — and your own are registered on top.
pub fn commands() -> Console {
    rainier_framework::console("app").register(SeedCommand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_ins_and_our_own_are_all_registered() {
        let console = commands();

        for name in ["serve", "route:list", "migrate", "queue:work", "app:seed"] {
            assert!(console.find(name).is_some(), "`{name}` should be registered");
        }
    }
}
