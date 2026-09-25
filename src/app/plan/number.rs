#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CanonicalNumber(String);

pub(super) fn canonical_number(value: &str) -> Option<CanonicalNumber> {
    let mut parser = NumberParser::new(value);
    let negative = parser.take_sign();
    let integer = parser.take_digits()?;
    let fraction = parser.take_fraction()?;
    let exponent = parser.take_exponent()?;
    if !parser.is_complete() {
        return None;
    }

    let mut digits = integer;
    digits.push_str(&fraction);
    let first_non_zero = digits
        .bytes()
        .position(|digit| digit != b'0')
        .unwrap_or(digits.len());
    if first_non_zero == digits.len() {
        return Some(CanonicalNumber("0".to_owned()));
    }
    digits.drain(..first_non_zero);

    let trailing_zero_count = digits
        .bytes()
        .rev()
        .take_while(|digit| *digit == b'0')
        .count();
    digits.truncate(digits.len() - trailing_zero_count);

    let mut scale = exponent;
    scale.adjust(false, fraction.len());
    scale.adjust(true, trailing_zero_count);

    let sign = if negative { "-" } else { "" };
    Some(CanonicalNumber(format!(
        "{sign}{digits}e{}",
        scale.as_string()
    )))
}

struct NumberParser<'a> {
    input: &'a [u8],
    position: usize,
}

impl<'a> NumberParser<'a> {
    const fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            position: 0,
        }
    }

    fn take_sign(&mut self) -> bool {
        if self.input.get(self.position) == Some(&b'-') {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn take_digits(&mut self) -> Option<String> {
        let start = self.position;
        while self
            .input
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        (self.position > start)
            .then(|| String::from_utf8_lossy(&self.input[start..self.position]).into())
    }

    fn take_fraction(&mut self) -> Option<String> {
        if self.input.get(self.position) != Some(&b'.') {
            return Some(String::new());
        }
        self.position += 1;
        self.take_digits()
    }

    fn take_exponent(&mut self) -> Option<DecimalExponent> {
        if !matches!(self.input.get(self.position), Some(b'e' | b'E')) {
            return Some(DecimalExponent::zero());
        }
        self.position += 1;
        let negative = match self.input.get(self.position) {
            Some(b'-') => {
                self.position += 1;
                true
            }
            Some(b'+') => {
                self.position += 1;
                false
            }
            _ => false,
        };
        let digits = self.take_digits()?;
        Some(DecimalExponent::new(negative, &digits))
    }

    const fn is_complete(&self) -> bool {
        self.position == self.input.len()
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DecimalExponent {
    negative: bool,
    digits: String,
}

impl DecimalExponent {
    fn new(negative: bool, digits: &str) -> Self {
        let first_non_zero = digits
            .bytes()
            .position(|digit| digit != b'0')
            .unwrap_or(digits.len());
        if first_non_zero == digits.len() {
            return Self::zero();
        }
        Self {
            negative,
            digits: digits[first_non_zero..].to_owned(),
        }
    }

    const fn zero() -> Self {
        Self {
            negative: false,
            digits: String::new(),
        }
    }

    fn adjust(&mut self, positive: bool, amount: usize) {
        if amount == 0 {
            return;
        }
        let amount = amount.to_string();
        if self.digits.is_empty() {
            self.negative = !positive;
            self.digits = amount;
            return;
        }
        if self.negative != positive {
            self.digits = add_decimal_digits(&self.digits, &amount);
            return;
        }
        match self
            .digits
            .len()
            .cmp(&amount.len())
            .then_with(|| self.digits.cmp(&amount))
        {
            std::cmp::Ordering::Greater => {
                self.digits = subtract_decimal_digits(&self.digits, &amount);
            }
            std::cmp::Ordering::Equal => {
                self.digits.clear();
                self.negative = false;
            }
            std::cmp::Ordering::Less => {
                self.digits = subtract_decimal_digits(&amount, &self.digits);
                self.negative = !self.negative;
            }
        }
    }

    fn as_string(&self) -> String {
        if self.digits.is_empty() {
            "0".to_owned()
        } else if self.negative {
            format!("-{}", self.digits)
        } else {
            self.digits.clone()
        }
    }
}

fn add_decimal_digits(left: &str, right: &str) -> String {
    let mut result = Vec::with_capacity(left.len().max(right.len()) + 1);
    let mut carry = 0u8;
    let mut left = left.bytes().rev();
    let mut right = right.bytes().rev();
    loop {
        let left_digit = left.next();
        let right_digit = right.next();
        if left_digit.is_none() && right_digit.is_none() {
            if carry != 0 {
                result.push(b'1');
            }
            result.reverse();
            return String::from_utf8(result).expect("decimal digits are valid UTF-8");
        }
        let sum = left_digit.map_or(0, |digit| digit - b'0')
            + right_digit.map_or(0, |digit| digit - b'0')
            + carry;
        result.push(b'0' + sum % 10);
        carry = sum / 10;
    }
}

fn subtract_decimal_digits(left: &str, right: &str) -> String {
    let mut result = Vec::with_capacity(left.len());
    let mut borrow = 0i16;
    let left = left.bytes().rev();
    let mut right = right.bytes().rev();
    for left_digit in left {
        let mut difference = i16::from(left_digit - b'0') - borrow;
        let right_digit = right.next().map_or(0, |digit| digit - b'0');
        difference -= i16::from(right_digit);
        if difference < 0 {
            difference += 10;
            borrow = 1;
        } else {
            borrow = 0;
        }
        result.push(
            b'0' + u8::try_from(difference).expect("decimal subtraction stays within one digit"),
        );
    }
    while result.last() == Some(&b'0') {
        result.pop();
    }
    result.reverse();
    String::from_utf8(result).expect("decimal digits are valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::canonical_number;

    struct NumberCase {
        name: &'static str,
        input: &'static str,
        expected: &'static str,
    }

    #[test]
    fn canonicalizes_large_decimal_values_without_float_conversion() {
        let cases = [
            NumberCase {
                name: "preserves integers above 2^53",
                input: "9007199254740993",
                expected: "9007199254740993e0",
            },
            NumberCase {
                name: "removes fractional trailing zeros",
                input: "-12.300e2",
                expected: "-123e1",
            },
            NumberCase {
                name: "normalizes zero with sign and exponent",
                input: "-0.000e+99",
                expected: "0",
            },
            NumberCase {
                name: "retains arbitrary exponent precision",
                input: "1e999999999999999999",
                expected: "1e999999999999999999",
            },
        ];

        for case in cases {
            let actual = canonical_number(case.input).map(|number| number.0);
            assert_eq!(
                actual.as_deref(),
                Some(case.expected),
                "case: {}",
                case.name
            );
        }
    }
}
