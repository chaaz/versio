//! A sum type for different kinds of iterators.

pub enum IterEither2<A, B> {
  A(A),
  B(B)
}

impl<A, B> Iterator for IterEither2<A, B>
where
  A: Iterator,
  B: Iterator<Item = A::Item>
{
  type Item = A::Item;

  fn next(&mut self) -> Option<A::Item> {
    match self {
      IterEither2::A(a) => a.next(),
      IterEither2::B(b) => b.next()
    }
  }
}
