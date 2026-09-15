use std::future::{Future, poll_fn};
use std::pin::{Pin, pin};
use std::task::{Context, Poll};

pub(super) enum First<A, B> {
    Left(A),
    Right(B),
}

fn poll<A: Future, B: Future>(
    left: Pin<&mut A>,
    right: Pin<&mut B>,
    cx: &mut Context<'_>,
) -> Poll<First<A::Output, B::Output>> {
    match left.poll(cx) {
        Poll::Ready(value) => Poll::Ready(First::Left(value)),
        Poll::Pending => right.poll(cx).map(First::Right),
    }
}

pub(super) async fn first<A: Future, B: Future>(left: A, right: B) -> First<A::Output, B::Output> {
    let (mut left, mut right) = (pin!(left), pin!(right));
    poll_fn(|cx| poll(left.as_mut(), right.as_mut(), cx)).await
}
