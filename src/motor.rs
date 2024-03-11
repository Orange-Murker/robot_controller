use esp_idf_svc::{
    hal::{gpio::*, ledc::LedcDriver},
    sys::EspError,
};

pub struct MotorControl<'a> {
    // left_step: AnyOutputPin,
    pub left_step: LedcDriver<'a>,
    pub left_dir: PinDriver<'a, Gpio20, Output>,
    pub right_step: LedcDriver<'a>,
    pub right_dir: PinDriver<'a, Gpio21, Output>,
}

pub enum Direction {
    Forward,
    Back,
    Left,
    Right,
}

impl<'a> MotorControl<'a> {
    pub fn set_enable(&mut self, enable: bool) -> Result<(), EspError> {
        if enable {
            self.left_step.enable()?;
            self.right_step.enable()?;
        } else {
            self.left_step.disable()?;
            self.right_step.disable()?;
        }

        Ok(())
    }

    pub fn set_direction(&mut self, direction: Direction) -> Result<(), EspError> {
        match direction {
            Direction::Forward => {
                self.left_dir.set_high()?;
                self.right_dir.set_high()?;
            }
            Direction::Back => {
                self.left_dir.set_low()?;
                self.right_dir.set_low()?;
            }
            Direction::Left => {
                self.left_dir.set_low()?;
                self.right_dir.set_high()?;
            }
            Direction::Right => {
                self.left_dir.set_high()?;
                self.right_dir.set_low()?;
            }
        }

        Ok(())
    }
}
