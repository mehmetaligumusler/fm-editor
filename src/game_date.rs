//! Calendar-aware ages from FM's DateDayYear encoding.

/// A one-based day of the year and its Gregorian year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameDate {
    pub day: u16,
    pub year: u16,
}

impl GameDate {
    /// Decode an exact four-byte little-endian DateDayYear buffer.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let bytes: [u8; 4] = bytes.try_into().ok()?;
        Self::from_encoded(
            u16::from_le_bytes([bytes[0], bytes[1]]),
            u16::from_le_bytes([bytes[2], bytes[3]]),
        )
    }

    /// Decode the date words, removing FM's flags above the low nine day bits.
    pub(crate) fn from_encoded(day: u16, year: u16) -> Option<Self> {
        let date = Self {
            day: day & 0x1FF,
            year,
        };
        if !(1900..=2200).contains(&year) {
            return None;
        }
        month_day(date.day, year)?;
        Some(date)
    }
}

/// (day, month, year) from a packed DateDayYear pair with flag bits.
pub fn dmy_from_packed(doy_flags: u16, year: u16) -> Option<(u16, u8, u16)> {
    let date = GameDate::from_encoded(doy_flags, year)?;
    let (month, day) = month_day(date.day, date.year)?;
    Some((day as u16, month, date.year))
}

fn month_day(day: u16, year: u16) -> Option<(u8, u16)> {
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut remaining = day & 0x1FF;
    if remaining == 0 {
        return None;
    }
    for (index, length) in months.into_iter().enumerate() {
        if remaining <= length {
            return Some((index as u8 + 1, remaining));
        }
        remaining -= length;
    }
    None
}

/// Completed years of age on the game date. Invalid or future DOBs return None.
/// A February 29 birthday is reached on March 1 in a non-leap year.
pub fn age_on(dob: (u16, u16), today: GameDate) -> Option<u8> {
    let today = GameDate::from_encoded(today.day, today.year)?;
    let (day, year) = dob;
    if year < 1900 {
        return None;
    }
    let birthday = month_day(day, year)?;
    let current = month_day(today.day, today.year)?;
    let years = today.year.checked_sub(year)?;
    let age = years.checked_sub(u16::from(current < birthday))?;
    u8::try_from(age).ok().filter(|age| *age <= 120)
}

/// Use the game date when available, otherwise preserve the historical
/// reference-year estimate and its validation (including day 366).
pub fn age_with_reference(
    dob: (u16, u16),
    today: Option<GameDate>,
    reference_year: u16,
) -> Option<u8> {
    if let Some(today) = today {
        return age_on(dob, today);
    }
    let (day, year) = dob;
    if !(1..=366).contains(&(day & 0x1FF)) || year < 1900 {
        return None;
    }
    u8::try_from(reference_year.checked_sub(year)?)
        .ok()
        .filter(|age| *age <= 120)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn birthday_passed_not_yet_and_today() {
        let today = GameDate {
            day: 195,
            year: 2026,
        }; // July 14
        assert_eq!(age_on((189, 1991), today), Some(35)); // Van Dijk: July 8
        assert_eq!(age_on((200, 1991), today), Some(34));
        assert_eq!(age_on((195, 1991), today), Some(35));
        assert_eq!(age_on((85, 1982), today), Some(44)); // Arteta: March 26
    }

    #[test]
    fn leap_day_birthday_uses_calendar_month_and_day() {
        let dob = (60, 2000); // February 29
        assert_eq!(
            age_on(
                dob,
                GameDate {
                    day: 59,
                    year: 2025
                }
            ),
            Some(24)
        );
        assert_eq!(
            age_on(
                dob,
                GameDate {
                    day: 60,
                    year: 2025
                }
            ),
            Some(25)
        );
        assert_eq!(
            age_on(
                dob,
                GameDate {
                    day: 60,
                    year: 2024
                }
            ),
            Some(24)
        );
        // July 14 differs by one day-of-year across leap/non-leap years.
        assert_eq!(
            age_on(
                (196, 2000),
                GameDate {
                    day: 195,
                    year: 2026
                }
            ),
            Some(26)
        );
    }

    #[test]
    fn day_flags_are_masked() {
        let today = GameDate {
            day: 195 | 0x8000,
            year: 2026,
        };
        assert_eq!(age_on((189 | 0xFE00, 1991), today), Some(35));
        assert_eq!(
            GameDate::from_encoded(195 | 0x8000, 2026),
            Some(GameDate {
                day: 195,
                year: 2026
            })
        );
    }

    #[test]
    fn date_buffer_decoder_checks_length_values_and_flags() {
        for bytes in [&[][..], &[0][..], &[0, 0, 0][..], &[0, 0, 0, 0, 0][..]] {
            assert_eq!(GameDate::from_bytes(bytes), None);
        }
        for (day, year) in [
            (0u16, 2026u16),
            (367, 2026),
            (366, 2026),
            (1, 1899),
            (1, 2201),
        ] {
            let mut bytes = [0; 4];
            bytes[..2].copy_from_slice(&day.to_le_bytes());
            bytes[2..].copy_from_slice(&year.to_le_bytes());
            assert_eq!(GameDate::from_bytes(&bytes), None);
        }
        let mut bytes = [0; 4];
        bytes[..2].copy_from_slice(&(195u16 | 0xFE00).to_le_bytes());
        bytes[2..].copy_from_slice(&2026u16.to_le_bytes());
        assert_eq!(
            GameDate::from_bytes(&bytes),
            Some(GameDate {
                day: 195,
                year: 2026
            })
        );
    }

    #[test]
    fn invalid_dates_future_births_and_excessive_ages_are_rejected() {
        let today = GameDate {
            day: 195,
            year: 2026,
        };
        for dob in [
            (0, 1991),
            (367, 1991),
            (366, 1991),
            (195, 1899),
            (1, 2027),
            (196, 2026),
            (1, 1900),
        ] {
            assert_eq!(age_on(dob, today), None, "{dob:?}");
        }
        for date in [
            GameDate { day: 0, year: 2026 },
            GameDate {
                day: 366,
                year: 2026,
            },
            GameDate { day: 1, year: 1899 },
            GameDate { day: 1, year: 2201 },
        ] {
            assert_eq!(age_on((189, 1991), date), None, "{date:?}");
        }
        assert_eq!(age_on((195, 2026), today), Some(0));
        assert_eq!(age_on((195, 1906), today), Some(120));
    }

    #[test]
    fn gregorian_century_rules() {
        assert_eq!(month_day(366, 1900), None);
        assert_eq!(month_day(366, 2000), Some((12, 31)));
        assert_eq!(month_day(366, 2100), None);
    }

    #[test]
    fn absent_game_date_preserves_reference_year_estimate() {
        for year in 1899..=2028 {
            for day in [0, 1, 189, 366, 367] {
                let old = if (1..=366).contains(&day) && year >= 1900 {
                    2027u16
                        .checked_sub(year)
                        .and_then(|age| u8::try_from(age).ok())
                        .filter(|age| *age <= 120)
                } else {
                    None
                };
                assert_eq!(age_with_reference((day, year), None, 2027), old);
            }
        }
        assert_eq!(
            age_with_reference((189 | 0x8000, 1991), None, 2027),
            Some(36)
        );
        assert_eq!(
            age_with_reference(
                (189, 1991),
                Some(GameDate {
                    day: 195,
                    year: 2026
                }),
                2027
            ),
            Some(35)
        );
        // Invalid supplied dates do not silently become plausible fallback ages.
        assert_eq!(
            age_with_reference((189, 1991), Some(GameDate { day: 0, year: 2026 }), 2027),
            None
        );
    }
}
