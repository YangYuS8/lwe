//! Power management utilities
//!
//! Battery status detection and power saving features.

use std::fs;
use std::path::Path;

/// Power state detection
pub struct PowerManager {
    /// Cache for battery status
    on_battery: Option<bool>,
    /// Last check timestamp
    last_check: std::time::Instant,
    /// Check interval (5 seconds)
    check_interval: std::time::Duration,
}

impl PowerManager {
    pub fn new() -> Self {
        Self {
            on_battery: None,
            last_check: std::time::Instant::now(),
            check_interval: std::time::Duration::from_secs(5),
        }
    }

    /// Check if system is running on battery
    pub fn is_on_battery(&mut self) -> bool {
        // Only check every N seconds to avoid excessive I/O
        if self.last_check.elapsed() < self.check_interval {
            return self.on_battery.unwrap_or(false);
        }

        self.last_check = std::time::Instant::now();

        // Try to detect battery status
        let on_battery = Self::detect_battery_status();
        self.on_battery = Some(on_battery);
        on_battery
    }

    /// Detect if running on battery by checking /sys/class/power_supply
    fn detect_battery_status() -> bool {
        let power_supply_path = Path::new("/sys/class/power_supply");

        if !power_supply_path.exists() {
            return false;
        }

        // Iterate through power supply devices
        if let Ok(entries) = fs::read_dir(power_supply_path) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Check if it's a battery (BAT0, BAT1, etc.)
                if let Some(name) = path.file_name().and_then(|n| n.to_str())
                    && name.starts_with("BAT")
                {
                    // Check status file
                    let status_path = path.join("status");
                    if let Ok(status) = fs::read_to_string(status_path) {
                        let status = status.trim();
                        // "Discharging" means on battery
                        if status == "Discharging" {
                            return true;
                        }
                    }
                }
            }
        }

        false
    }
}

impl Default for PowerManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_battery_detection() {
        let mut pm = PowerManager::new();
        let on_battery = pm.is_on_battery();
        // Just ensure it doesn't panic
        println!("On battery: {}", on_battery);
    }
}
