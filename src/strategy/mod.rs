pub mod get_fastest;
pub mod get_first_healthy;

pub use get_fastest::get_fastest;
pub use get_first_healthy::get_first_healthy;

#[derive(Debug, Clone)]
pub enum Strategy {
    Fastest,
    FirstHealthy,
}
