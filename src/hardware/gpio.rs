use anyhow::{Context, Result};
use rppal::gpio::{Gpio, InputPin, Level, OutputPin};
use std::time::{Duration, Instant};

/// Button events that can be detected
#[derive(Debug, Clone)]
pub enum ButtonEvent {
    Pressed,
    Released(Duration),
}

/// Trait for reading button events
pub trait ButtonReader {
    fn check_event(&mut self) -> Option<ButtonEvent>;
}

/// Trait for controlling LEDs
pub trait LedController {
    fn turn_on(&mut self) -> Result<()>;
    fn turn_off(&mut self) -> Result<()>;
    fn is_on(&self) -> bool;
}

/// GPIO-based button reader implementation
pub struct GpioButtonReader {
    pin: InputPin,
    last_level: Level,
    press_start: Option<Instant>,
    action_handled: bool,
}

impl GpioButtonReader {
    /// Create a new GPIO button reader
    pub fn new(pin_number: u8) -> Result<Self> {
        let gpio = Gpio::new().context("Failed to initialize GPIO")?;
        let pin = gpio
            .get(pin_number)
            .with_context(|| format!("Failed to get GPIO pin {}", pin_number))?
            .into_input_pullup();
        
        let last_level = pin.read();
        
        Ok(Self {
            pin,
            last_level,
            press_start: None,
            action_handled: false,
        })
    }
    
    /// Check for button state changes and return events
    pub fn poll(&mut self) -> Option<ButtonEvent> {
        let current_level = self.pin.read();
        
        // Button is pressed when level goes from High to Low (pull-up configuration)
        if self.last_level == Level::High && current_level == Level::Low {
            // Button just pressed
            self.press_start = Some(Instant::now());
            self.action_handled = false;
            self.last_level = current_level;
            return Some(ButtonEvent::Pressed);
        }
        
        // Button is released when level goes from Low to High
        if self.last_level == Level::Low && current_level == Level::High {
            // Button just released
            if let Some(start) = self.press_start.take() {
                let duration = start.elapsed();
                self.last_level = current_level;
                self.action_handled = false;
                return Some(ButtonEvent::Released(duration));
            }
        }
        
        self.last_level = current_level;
        None
    }
    

}

impl ButtonReader for GpioButtonReader {
    fn check_event(&mut self) -> Option<ButtonEvent> {
        self.poll()
    }
}

/// GPIO-based LED controller implementation
pub struct GpioLedController {
    pin: OutputPin,
    is_on: bool,
}

impl GpioLedController {
    /// Create a new GPIO LED controller
    pub fn new(pin_number: u8) -> Result<Self> {
        let gpio = Gpio::new().context("Failed to initialize GPIO")?;
        let mut pin = gpio
            .get(pin_number)
            .with_context(|| format!("Failed to get GPIO pin {}", pin_number))?
            .into_output();
        
        // Start with LED off
        pin.set_low();
        
        Ok(Self {
            pin,
            is_on: false,
        })
    }
    
    /// Set LED to specific state
    pub fn set_state(&mut self, on: bool) -> Result<()> {
        if on {
            self.pin.set_high();
        } else {
            self.pin.set_low();
        }
        self.is_on = on;
        Ok(())
    }
    

}

impl LedController for GpioLedController {
    fn turn_on(&mut self) -> Result<()> {
        self.set_state(true)
    }
    
    fn turn_off(&mut self) -> Result<()> {
        self.set_state(false)
    }
    
    fn is_on(&self) -> bool {
        self.is_on
    }
    

}

/// Blink an LED for a specified duration
pub async fn blink_led(led: &mut dyn LedController, duration: Duration) -> Result<()> {
    let original_state = led.is_on();
    
    // Turn on
    led.turn_on()?;
    
    // Wait for specified duration
    tokio::time::sleep(duration).await;
    
    // Restore original state
    if original_state {
        led.turn_on()?;
    } else {
        led.turn_off()?;
    }
    
    Ok(())
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;



    #[test]
    fn test_button_event_detection() {
        // Note: This test would need to be adapted for actual GPIO testing
        // For now, we'll test the logic structure
        let event = ButtonEvent::Pressed;
        assert!(matches!(event, ButtonEvent::Pressed));
        
        let event = ButtonEvent::Released(Duration::from_secs(1));
        match event {
            ButtonEvent::Released(duration) => {
                assert_eq!(duration, Duration::from_secs(1));
            }
            _ => panic!("Expected Released event"),
        }
    }

    #[test]
    fn test_led_controller_trait() {
        // Test that we can create a basic mock LED controller
        struct MockLed {
            is_on: bool,
        }
        
        impl LedController for MockLed {
            fn turn_on(&mut self) -> Result<()> {
                self.is_on = true;
                Ok(())
            }
            
            fn turn_off(&mut self) -> Result<()> {
                self.is_on = false;
                Ok(())
            }
            
            fn is_on(&self) -> bool {
                self.is_on
            }
            

        }
        
        let mut led = MockLed { is_on: false };
        assert!(!led.is_on());
        
        led.turn_on().unwrap();
        assert!(led.is_on());
        
        led.turn_off().unwrap();
        assert!(!led.is_on());
    }


}