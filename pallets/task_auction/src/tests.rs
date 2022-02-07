use crate::{mock::*, Error};
use frame_support::{assert_err, assert_ok, BoundedVec};

fn last_event() -> Event {
	System::events().pop().expect("Event expected").event
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

		// check successful creation
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 5, test_data.clone()));
		if let Event::TaskAuction(crate::Event::<Test>::Created { auction_key, bounty, deadline }) =
			last_event()
		{
			assert_eq!(bounty, 1000);
			assert_eq!(deadline, 5);
			assert_eq!(Balances::reserved_balance(&0xA), 1500);

			let auction = TaskAuction::auctions(auction_key).unwrap();
			assert_eq!(auction_key.0, 0xA);
			assert_eq!(auction.arbitrator, 0xB);
			assert_eq!(auction.bounty, 1000);
			assert_eq!(auction.deposit, 500);
			assert_eq!(auction.deadline, 5);
			assert_eq!(auction.data, vec![1, 2, 3]);
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_none());
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
			TaskAuction::bid(Origin::signed(0xA), (1, 1), 100),
			Error::<Test>::AuctionIdNotFound
		);
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 5, test_data));
		if let Event::TaskAuction(crate::Event::<Test>::Created {
			auction_key,
			bounty: _,
			deadline: _,
		}) = last_event()
		{
			System::set_block_number(6);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xC), auction_key, 100),
				Error::<Test>::DeadlineExpired
			);
			System::set_block_number(1);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xA), auction_key, 100),
				Error::<Test>::BidderIsEmployer
			);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xB), auction_key, 100),
				Error::<Test>::BidderIsArbitrator
			);

			assert!(TaskAuction::bids(auction_key, (0, 0)).is_none());
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, 200));
			assert_eq!(Balances::reserved_balance(&0xC), 500);
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_some());

			assert_err!(
				TaskAuction::bid(Origin::signed(0xD), auction_key, 300),
				Error::<Test>::MinBidRatioRequired
			);
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_some());

			for i in 1..10 {
				let price = (200 - i) as u128;
				assert_ok!(TaskAuction::bid(Origin::signed(0xD), auction_key, price));
				assert_eq!(TaskAuction::bids(auction_key, (0, 0)).unwrap().1, price);
			}
		}
	})
}
