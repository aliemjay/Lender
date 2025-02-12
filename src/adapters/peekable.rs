use core::{fmt, ops::ControlFlow};

use crate::{
    try_trait_v2::{FromResidual, Try},
    DoubleEndedLender, ExactSizeLender, FusedLender, Lend, Lender, Lending,
};

#[must_use = "lenders are lazy and do nothing unless consumed"]
pub struct Peekable<'this, L>
where
    L: Lender,
{
    lender: L,
    peeked: Option<Option<<L as Lending<'this>>::Lend>>,
}
impl<'this, L> Peekable<'this, L>
where
    L: Lender,
{
    pub(crate) fn new(lender: L) -> Peekable<'this, L> {
        Peekable { lender, peeked: None }
    }
    pub fn peek(&mut self) -> Option<&'_ <L as Lending<'this>>::Lend> {
        let lender = &mut self.lender;
        self.peeked
            .get_or_insert_with(|| {
                // SAFETY: The lend is manually guaranteed to be the only one alive
                unsafe {
                    core::mem::transmute::<Option<<L as Lending<'_>>::Lend>, Option<<L as Lending<'this>>::Lend>>(
                        lender.next(),
                    )
                }
            })
            .as_ref()
    }
    pub fn peek_mut(&mut self) -> Option<&'_ mut <L as Lending<'this>>::Lend> {
        let lender = &mut self.lender;
        self.peeked
            .get_or_insert_with(|| {
                // SAFETY: The lend is manually guaranteed to be the only one alive
                unsafe {
                    core::mem::transmute::<Option<<L as Lending<'_>>::Lend>, Option<<L as Lending<'this>>::Lend>>(
                        lender.next(),
                    )
                }
            })
            .as_mut()
    }
    pub fn next_if<F>(&mut self, f: F) -> Option<<L as Lending<'_>>::Lend>
    where
        F: FnOnce(&<L as Lending<'_>>::Lend) -> bool,
    {
        let peeked = unsafe { &mut *(&mut self.peeked as *mut _) };
        match self.next() {
            Some(v) if f(&v) => Some(v),
            v => {
                // SAFETY: The lend is manually guaranteed to be the only one alive
                *peeked = Some(unsafe {
                    core::mem::transmute::<Option<<L as Lending<'_>>::Lend>, Option<<L as Lending<'this>>::Lend>>(v)
                });
                None
            }
        }
    }
    pub fn next_if_eq<'a, T>(&'a mut self, t: &T) -> Option<<L as Lending<'a>>::Lend>
    where
        T: for<'all> PartialEq<<L as Lending<'all>>::Lend>,
    {
        self.next_if(|v| t == v)
    }
}
impl<'this, L> Clone for Peekable<'this, L>
where
    L: Lender + Clone,
{
    fn clone(&self) -> Self {
        Peekable { lender: self.lender.clone(), peeked: None }
    }
}
impl<'this, L: fmt::Debug> fmt::Debug for Peekable<'this, L>
where
    L: Lender + fmt::Debug,
    <L as Lending<'this>>::Lend: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Peekable").field("lender", &self.lender).field("peeked", &self.peeked).finish()
    }
}
impl<'lend, 'this, L> Lending<'lend> for Peekable<'this, L>
where
    L: Lender,
{
    type Lend = Lend<'lend, L>;
}
impl<'this, L> Lender for Peekable<'this, L>
where
    L: Lender,
{
    fn next(&mut self) -> Option<<Self as Lending<'_>>::Lend> {
        match self.peeked.take() {
            // SAFETY: The lend is manually guaranteed to be the only one alive
            Some(peeked) => unsafe {
                core::mem::transmute::<Option<<Self as Lending<'this>>::Lend>, Option<<Self as Lending<'_>>::Lend>>(peeked)
            },
            None => self.lender.next(),
        }
    }
    #[inline]
    fn count(mut self) -> usize {
        match self.peeked.take() {
            Some(None) => 0,
            Some(Some(_)) => 1 + self.lender.count(),
            None => self.lender.count(),
        }
    }
    #[inline]
    fn nth(&mut self, n: usize) -> Option<<Self as Lending<'_>>::Lend> {
        match self.peeked.take() {
            Some(None) => None,
            // SAFETY: The lend is manually guaranteed to be the only one alive
            Some(v @ Some(_)) if n == 0 => unsafe {
                core::mem::transmute::<Option<<Self as Lending<'this>>::Lend>, Option<<Self as Lending<'_>>::Lend>>(v)
            },
            Some(Some(_)) => self.lender.nth(n - 1),
            None => self.lender.nth(n),
        }
    }
    #[inline]
    fn last<'a>(mut self) -> Option<<Self as Lending<'a>>::Lend>
    where
        Self: Sized + 'a,
    {
        let peek_opt = match self.peeked.take() {
            Some(None) => return None,
            // SAFETY: 'this: 'call
            Some(v) => unsafe {
                core::mem::transmute::<Option<<Self as Lending<'this>>::Lend>, Option<<Self as Lending<'a>>::Lend>>(v)
            },
            None => None,
        };
        self.lender.last().or(peek_opt)
    }
    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let peek_len = match self.peeked {
            Some(None) => return (0, Some(0)),
            Some(Some(_)) => 1,
            None => 0,
        };
        let (l, r) = self.lender.size_hint();
        (l.saturating_add(peek_len), r.and_then(|r| r.checked_add(peek_len)))
    }
    #[inline]
    fn try_fold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        Self: Sized,
        F: FnMut(B, <Self as Lending<'_>>::Lend) -> R,
        R: Try<Output = B>,
    {
        let acc = match self.peeked.take() {
            Some(None) => return Try::from_output(init),
            Some(Some(v)) => match f(init, v).branch() {
                ControlFlow::Break(b) => return FromResidual::from_residual(b),
                ControlFlow::Continue(a) => a,
            },
            None => init,
        };
        self.lender.try_fold(acc, f)
    }
    #[inline]
    fn fold<B, F>(mut self, init: B, mut f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, <Self as Lending<'_>>::Lend) -> B,
    {
        let acc = match self.peeked.take() {
            Some(None) => return init,
            Some(Some(v)) => f(init, v),
            None => init,
        };
        self.lender.fold(acc, f)
    }
}
impl<'this, L: DoubleEndedLender> DoubleEndedLender for Peekable<'this, L> {
    #[inline]
    fn next_back(&mut self) -> Option<<Self as Lending<'_>>::Lend> {
        match self.peeked.as_mut() {
            // SAFETY: The lend is manually guaranteed to be the only one alive
            Some(v @ Some(_)) => self.lender.next_back().or_else(|| unsafe {
                core::mem::transmute::<Option<<Self as Lending<'this>>::Lend>, Option<<Self as Lending<'_>>::Lend>>(v.take())
            }),
            Some(None) => None,
            None => self.lender.next_back(),
        }
    }
    #[inline]
    fn try_rfold<B, F, R>(&mut self, init: B, mut f: F) -> R
    where
        Self: Sized,
        F: FnMut(B, <Self as Lending<'_>>::Lend) -> R,
        R: Try<Output = B>,
    {
        match self.peeked.take() {
            None => self.lender.try_rfold(init, f),
            Some(None) => Try::from_output(init),
            Some(Some(v)) => match self.lender.try_rfold(init, &mut f).branch() {
                ControlFlow::Continue(acc) => f(acc, v),
                ControlFlow::Break(r) => {
                    self.peeked = Some(Some(v));
                    FromResidual::from_residual(r)
                }
            },
        }
    }
    #[inline]
    fn rfold<B, F>(mut self, init: B, mut f: F) -> B
    where
        Self: Sized,
        F: FnMut(B, <Self as Lending<'_>>::Lend) -> B,
    {
        match self.peeked.take() {
            None => self.lender.rfold(init, f),
            Some(None) => init,
            Some(Some(v)) => {
                let acc = self.lender.rfold(init, &mut f);
                f(acc, v)
            }
        }
    }
}
impl<'this, L: ExactSizeLender> ExactSizeLender for Peekable<'this, L> {}

impl<'this, L: FusedLender> FusedLender for Peekable<'this, L> {}
