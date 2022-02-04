use crate::{mock::*, Error};
use frame_support::{assert_err, assert_noop, assert_ok, BoundedVec};

fn last_event() -> Event {
	System::events().pop().expect("Event expected").event
}

#[test]
fn it_works_for_default_value() {
	new_test_ext().execute_with(|| {
		// Dispatch a signed extrinsic.
		assert_ok!(TaskAuction::do_something(Origin::signed(1), 42));
		// Read pallet storage and assert an expected result.
		assert_eq!(TaskAuction::something(), Some(42));
	});
}

#[test]
fn correct_error_for_none_value() {
	new_test_ext().execute_with(|| {
		// Ensure the expected error is thrown when no value is present.
		assert_noop!(TaskAuction::cause_error(Origin::signed(1)), Error::<Test>::NoneValue);
	});
}

#[test]
fn new_test_ext_behaves() {
	new_test_ext().execute_with(|| {
		assert_eq!(Balances::free_balance(&0xA), 10000);
	})
}

#[test]
fn create() {
	new_test_ext().execute_with(|| {
		let test_data: BoundedVec<u8, <Test as crate::Config>::MaxDataSize> =
			vec![1, 2, 3].try_into().unwrap();
		assert_err!(
			TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 0, test_data.clone()),
			Error::<Test>::DeadlineExpired
		);
		assert_err!(
			TaskAuction::create(Origin::signed(0xA), 0xB, 100, 500, 5, test_data.clone()),
			Error::<Test>::MinBountyRequired
		);
		assert_err!(
			TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 50, 5, test_data.clone()),
			Error::<Test>::MinDepositRequired
		);
		assert_err!(
			TaskAuction::create(Origin::signed(0xA), 0xB, 20000, 500, 5, test_data.clone()),
			pallet_balances::Error::<Test>::InsufficientBalance
		);
		assert_err!(
			TaskAuction::create(Origin::signed(0xA), 0xB, 500, 20000, 5, test_data.clone()),
			pallet_balances::Error::<Test>::InsufficientBalance
		);
		assert_eq!(TaskAuction::auction_count(), 0);

		// check successful creation
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 5, test_data.clone()));
		assert_eq!(TaskAuction::auction_count(), 1);
		if let Event::TaskAuction(crate::Event::<Test>::Created { auction_id, bounty, deadline }) =
			last_event()
		{
			assert_eq!(bounty, 1000);
			assert_eq!(deadline, 5);
			assert_eq!(Balances::reserved_balance(&0xA), 1500);

			let auction = TaskAuction::auctions(auction_id).unwrap();
			assert_eq!(auction.employer, 0xA);
			assert_eq!(auction.arbitrator, 0xB);
			assert_eq!(auction.bounty, 1000);
			assert_eq!(auction.deposit, 500);
			assert_eq!(auction.deadline, 5);
			assert_eq!(auction.data, vec![1, 2, 3]);
			assert!(TaskAuction::bids(auction_id).is_empty());
		} else {
			panic!("wrong event type")
		}
	})
}

#[test]
fn bid() {
	new_test_ext().execute_with(|| {
		let test_data: BoundedVec<u8, <Test as crate::Config>::MaxDataSize> =
			vec![1, 2, 3].try_into().unwrap();
		assert_err!(
			TaskAuction::bid(Origin::signed(0xA), 100, 100),
			Error::<Test>::AuctionIdNotFound
		);
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 5, test_data));
		if let Event::TaskAuction(crate::Event::<Test>::Created {
			auction_id,
			bounty: _,
			deadline: _,
		}) = last_event()
		{
			System::set_block_number(6);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xC), auction_id, 100),
				Error::<Test>::DeadlineExpired
			);
			System::set_block_number(1);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xA), auction_id, 100),
				Error::<Test>::BidderIsEmployer
			);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xB), auction_id, 100),
				Error::<Test>::BidderIsArbitrator
			);

			assert!(TaskAuction::bids(auction_id).is_empty());
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_id, 200));
			assert_eq!(Balances::reserved_balance(&0xC), 500);
			assert_eq!(TaskAuction::bids(auction_id).len(), 1);

			assert_err!(
				TaskAuction::bid(Origin::signed(0xD), auction_id, 300),
				Error::<Test>::MinBidRatioRequired
			);
			assert_eq!(TaskAuction::bids(auction_id).len(), 1);

			for i in 1..<Test as crate::Config>::MaxBidCount::get() {
				let price: u128 = (200 - i).into();
				assert_ok!(TaskAuction::bid(Origin::signed(0xD), auction_id, price));
				assert_eq!(TaskAuction::bids(auction_id).last().unwrap().1, price);
				assert_eq!(TaskAuction::bids(auction_id).len(), (1 + i) as usize);
			}
			assert_err!(
				TaskAuction::bid(Origin::signed(0xD), auction_id, 10),
				Error::<Test>::MaxBidCountExceeded
			);
		}
	})
}
