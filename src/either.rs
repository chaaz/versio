//! A sum type for different kinds of iterators.

pub enum IterEither<A, B, C> {
  A(A),
  B(B),
  C(C)
}

impl<A, B, C> Iterator for IterEither<A, B, C>
where
  A: Iterator,
  B: Iterator<Item = A::Item>,
  C: Iterator<Item = A::Item>
{
  type Item = A::Item;

  fn next(&mut self) -> Option<A::Item> {
    match self {
      IterEither::A(a) => a.next(),
      IterEither::B(b) => b.next(),
      IterEither::C(c) => c.next()
    }
  }
}
