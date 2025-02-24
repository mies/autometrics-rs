use syn::parse::{Parse, ParseStream};
use syn::{Expr, ItemFn, ItemImpl, Result, Token};

/// Autometrics can be applied to individual functions or to
/// (all of the methods within) impl blocks.
pub(crate) enum Item {
    Function(ItemFn),
    Impl(ItemImpl),
}

impl Parse for Item {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(Token![impl]) {
            input.parse().map(Item::Impl)
        } else {
            input.parse().map(Item::Function)
        }
    }
}

#[derive(Default)]
pub(crate) struct Args {
    pub track_concurrency: bool,
    pub ok_if: Option<Expr>,
    pub error_if: Option<Expr>,

    #[cfg(feature = "alerts")]
    pub alerts: Option<alerts::Alerts>,
}

mod kw {
    syn::custom_keyword!(track_concurrency);
    syn::custom_keyword!(alerts);
    syn::custom_keyword!(success_rate);
    syn::custom_keyword!(latency);
    syn::custom_keyword!(ok_if);
    syn::custom_keyword!(error_if);
}

impl Parse for Args {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut args = Args::default();
        while !input.is_empty() {
            let lookahead = input.lookahead1();
            if lookahead.peek(kw::track_concurrency) {
                let _ = input.parse::<kw::track_concurrency>()?;
                args.track_concurrency = true;
            } else if lookahead.peek(kw::ok_if) {
                if args.ok_if.is_some() {
                    return Err(input.error("expected only a single `ok_if` argument"));
                }
                if args.error_if.is_some() {
                    return Err(input.error("cannot use both `ok_if` and `error_if`"));
                }
                let ok_if = input.parse::<ExprArg<kw::ok_if>>()?;
                args.ok_if = Some(ok_if.value);
            } else if lookahead.peek(kw::error_if) {
                if args.error_if.is_some() {
                    return Err(input.error("expected only a single `error_if` argument"));
                }
                if args.ok_if.is_some() {
                    return Err(input.error("cannot use both `ok_if` and `error_if`"));
                }
                let error_if = input.parse::<ExprArg<kw::error_if>>()?;
                args.error_if = Some(error_if.value);
            } else if lookahead.peek(kw::alerts) {
                #[cfg(feature = "alerts")]
                {
                    let _ = input.parse::<kw::alerts>()?;
                    args.alerts = Some(input.parse()?);
                }
                #[cfg(not(feature = "alerts"))]
                return Err(syn::Error::new(
                    input.span(),
                    "feature \"alerts\" is required to use alerts",
                ));
            } else if lookahead.peek(Token![,]) {
                let _ = input.parse::<Token![,]>()?;
            } else {
                return Err(lookahead.error());
            }
        }
        Ok(args)
    }
}

struct ExprArg<T> {
    value: Expr,
    _p: std::marker::PhantomData<T>,
}

impl<T: Parse> Parse for ExprArg<T> {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let _ = input.parse::<T>()?;
        let _ = input.parse::<Token![=]>()?;
        let value = input.parse()?;
        Ok(Self {
            value,
            _p: std::marker::PhantomData,
        })
    }
}

#[cfg(feature = "alerts")]
mod alerts {
    use super::*;
    use rust_decimal::Decimal;
    use syn::{LitFloat, LitInt};

    #[cfg(feature = "alerts")]
    #[derive(Default, Debug)]
    pub(crate) struct Alerts {
        pub success_rate: Option<Decimal>,
        pub latency: Option<Latency>,
    }

    // Parse alerts in the form alerts(success_rate = 99.9%, latency(99.9% < 200ms))
    impl Parse for Alerts {
        fn parse(input: ParseStream) -> Result<Self> {
            let content;
            let _ = syn::parenthesized!(content in input);

            let mut alerts = Alerts::default();
            while !content.is_empty() {
                let lookahead = content.lookahead1();
                if lookahead.peek(kw::success_rate) {
                    let _ = content.parse::<kw::success_rate>()?;

                    let _ = content.parse::<Token![=]>()?;

                    let success_rate = content.parse::<IntOrFloat>()?.0 / Decimal::from(100);
                    let _ = content.parse::<Token![%]>()?;

                    alerts.success_rate = Some(success_rate);
                } else if lookahead.peek(kw::latency) {
                    alerts.latency = Some(content.parse()?);
                } else if lookahead.peek(Token![,]) {
                    let _ = content.parse::<Token![,]>()?;
                } else {
                    return Err(lookahead.error());
                }
            }
            Ok(alerts)
        }
    }

    #[cfg(feature = "alerts")]
    #[derive(Debug)]
    pub(crate) struct Latency {
        pub target_seconds: Decimal,
        pub percentile: Decimal,
    }

    // Parse latency in the form latency(99.9% < 200ms)
    impl Parse for Latency {
        fn parse(input: ParseStream) -> Result<Self> {
            let _ = input.parse::<kw::latency>()?;
            let content;
            let _ = syn::parenthesized!(content in input);

            let percentile = content.parse::<IntOrFloat>()?.0 / Decimal::from(100);

            let _ = content.parse::<Token![%]>()?;
            // Handle if the next token is either: <, <=, or =
            let lookahead = content.lookahead1();
            if lookahead.peek(Token![<=]) {
                let _ = content.parse::<Token![<=]>()?;
            } else if lookahead.peek(Token![<]) {
                let _ = content.parse::<Token![<]>()?;
            } else if lookahead.peek(Token![=]) {
                let _ = content.parse::<Token![=]>()?;
            } else {
                return Err(lookahead.error());
            }

            let IntOrFloat(target_seconds, unit) = content.parse()?;
            let target_seconds = match unit {
                Some(Unit::Seconds) => target_seconds,
                Some(Unit::Milliseconds) => target_seconds / Decimal::from(1000),
                _ => return Err(content.error("expected unit of time (s or ms)")),
            };

            Ok(Latency {
                target_seconds,
                percentile,
            })
        }
    }

    #[cfg(feature = "alerts")]
    enum Unit {
        Seconds,
        Milliseconds,
    }

    #[cfg(feature = "alerts")]
    struct IntOrFloat(Decimal, Option<Unit>);

    impl Parse for IntOrFloat {
        fn parse(input: ParseStream) -> Result<Self> {
            let lookahead = input.lookahead1();
            let (float, suffix) = if lookahead.peek(syn::LitInt) {
                let lit_int: LitInt = input.parse()?;
                (lit_int.base10_parse()?, lit_int.suffix().to_string())
            } else if lookahead.peek(syn::LitFloat) {
                let lit_float: LitFloat = input.parse()?;
                (lit_float.base10_parse()?, lit_float.suffix().to_string())
            } else {
                return Err(lookahead.error());
            };

            let unit = match suffix.as_str() {
                "" => None,
                "ms" => Some(Unit::Milliseconds),
                "s" => Some(Unit::Seconds),
                _ => return Err(lookahead.error()),
            };
            Ok(IntOrFloat(float, unit))
        }
    }
}
