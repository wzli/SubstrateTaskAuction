use crate::{mock::*, Error};
use frame_support::{assert_err, assert_ok};

type AuctionEvent = crate::Event<Test>;

fn last_event() -> AuctionEvent {
	if let Event::TaskAuction(e) = System::events().pop().expect("Event expected").event {
		e
	} else {
		panic!("TaskAuction event expected");
	}
}

#[test]
fn create() {
	new_test_ext().execute_with(|| {
		let test_data = vec![1, 2, 3];
		assert_err!(
			TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 5, vec![0; 2000]),
			Error::<Test>::MaxDataSizeExceeded
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
		if let AuctionEvent::Created { auction_key, bounty, terminal_block } = last_event() {
			assert_eq!(bounty, 1000);
			assert_eq!(terminal_block, 5);
			assert_eq!(Balances::reserved_balance(&0xA), 1500);

			let auction = TaskAuction::auctions(auction_key).unwrap();
			assert_eq!(auction_key.0, 0xA);
			assert_eq!(auction.arbitrator, 0xB);
			assert_eq!(auction.bounty, 1000);
			assert_eq!(auction.deposit, 500);
			assert_eq!(auction.terminal_block, 5);
			assert_eq!(auction.data, vec![1, 2, 3]);
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_none());
		} else {
			panic!("wrong event type")
		}
	})
}

#[test]
fn extend() {
	new_test_ext().execute_with(|| {
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 5, vec![0; 8]));
		if let AuctionEvent::Created { auction_key, bounty: _, terminal_block: _ } = last_event() {
			// input checks
			assert_err!(
				TaskAuction::extend(Origin::signed(0xB), auction_key.clone(), 2000, 6),
				Error::<Test>::OwnerRequired
			);
			assert_eq!(Balances::reserved_balance(&0xA), 1500);
			assert_err!(
				TaskAuction::extend(Origin::signed(0xA), (0, 0), 2000, 6),
				Error::<Test>::AuctionKeyNotFound
			);
			assert_eq!(Balances::reserved_balance(&0xA), 1500);

			assert_err!(
				TaskAuction::extend(Origin::signed(0xA), auction_key.clone(), 500, 6),
				Error::<Test>::MinBountyRequired
			);
			assert_eq!(Balances::reserved_balance(&0xA), 1500);

			// make sucessful bids before extension
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, 900));
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, 850));
			assert_eq!(Balances::reserved_balance(&0xC), 500);

			// successful extension bumps up bounty and shortens deadline
			assert_ok!(TaskAuction::extend(Origin::signed(0xA), auction_key.clone(), 2000, 0));
			assert_eq!(Balances::reserved_balance(&0xA), 2500);

			// previous bid is already assigned after extension
			assert_err!(
				TaskAuction::bid(Origin::signed(0xC), auction_key, 800),
				Error::<Test>::AuctionAssigned
			);
			assert_err!(
				TaskAuction::extend(Origin::signed(0xA), auction_key.clone(), 3000, 6),
				Error::<Test>::AuctionAssigned
			);
		}
	});
}

#[test]
fn bid() {
	new_test_ext().execute_with(|| {
		let test_data = vec![1, 2, 3];
		assert_err!(
			TaskAuction::bid(Origin::signed(0xA), (1, 1), 100),
			Error::<Test>::AuctionKeyNotFound
		);
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, 500, 5, test_data));
		if let AuctionEvent::Created { auction_key, bounty: _, terminal_block: _ } = last_event() {
			assert_err!(
				TaskAuction::bid(Origin::signed(0xA), auction_key, 100),
				Error::<Test>::OriginProhibited
			);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xB), auction_key, 100),
				Error::<Test>::OriginProhibited
			);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xC), auction_key, 1100),
				Error::<Test>::MinBidRatioRequired
			);

			assert!(TaskAuction::bids(auction_key, (0, 0)).is_none());
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, 300));
			assert_eq!(Balances::reserved_balance(&0xC), 500);
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_some());

			assert_err!(
				TaskAuction::bid(Origin::signed(0xD), auction_key, 400),
				Error::<Test>::MinBidRatioRequired
			);
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_some());

			for i in 1..10 {
				let price = (300 - (i * 6)) as u128;
				assert_ok!(TaskAuction::bid(Origin::signed(0xD), auction_key, price));
				assert_eq!(TaskAuction::bids(auction_key, (0, 0)).unwrap().1, price);
				if let AuctionEvent::Bid { auction_key: _, bid_key, price: _ } = last_event() {
					assert_eq!(bid_key, (0xD, i + 1));
				}
			}
			assert_eq!(Balances::reserved_balance(&0xC), 0);
			assert_eq!(Balances::reserved_balance(&0xD), 500);
			System::set_block_number(3);
			assert_err!(
				TaskAuction::bid(Origin::signed(0xC), auction_key, 100),
				Error::<Test>::AuctionAssigned
			);
		}
	})
}

#[test]
fn retract() {
	new_test_ext().execute_with(|| {
		// no auction yet
		assert_err!(
			TaskAuction::retract(Origin::signed(0xC), (0, 0)),
			Error::<Test>::AuctionKeyNotFound
		);
		// create auction
		let deposit = 500;
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, deposit, 5, vec![0; 8]));
		if let AuctionEvent::Created { auction_key, bounty: _, terminal_block: _ } = last_event() {
			// insert 10 bids from C
			for i in 0..10 {
				let price = (500 - (i * 10)) as u128;
				assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, price));
				assert_eq!(Balances::reserved_balance(&0xC), deposit);
			}
			// insert 10 bids from D
			for i in 10..20 {
				let price = (500 - (i * 10)) as u128;
				assert_ok!(TaskAuction::bid(Origin::signed(0xD), auction_key, price));
				assert_eq!(Balances::reserved_balance(&0xD), deposit);
				assert_eq!(Balances::reserved_balance(&0xC), 0);
			}
			// C can't retract because top bid is from D
			assert_err!(
				TaskAuction::retract(Origin::signed(0xC), auction_key),
				Error::<Test>::TopBidRequired
			);

			// retract 10 bids from D
			assert_eq!(Balances::reserved_balance(&0xD), deposit);
			assert_eq!(Balances::reserved_balance(&0xC), 0);
			for _ in 0..10 {
				assert_ok!(TaskAuction::retract(Origin::signed(0xD), auction_key));
			}

			// retract 10 bids from C
			assert_eq!(Balances::reserved_balance(&0xC), deposit);
			assert_eq!(Balances::reserved_balance(&0xD), 0);
			for _ in 0..10 {
				assert_ok!(TaskAuction::retract(Origin::signed(0xC), auction_key));
			}
			assert_eq!(Balances::reserved_balance(&0xC), 0);
			assert_eq!(Balances::reserved_balance(&0xD), 0);
			assert_eq!(Balances::free_balance(&0xC), 10000);
			assert_eq!(Balances::free_balance(&0xD), 10000);

			// auction has no bids left to retract
			assert_err!(
				TaskAuction::retract(Origin::signed(0xB), auction_key),
				Error::<Test>::TopBidRequired
			);

			// assign auction to D
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, 900));
			assert_ok!(TaskAuction::bid(Origin::signed(0xD), auction_key, 800));
			System::set_block_number(10);

			// retracting bid from assigned auction results in losing deposit
			assert_ok!(TaskAuction::retract(Origin::signed(0xD), auction_key));
			assert_eq!(Balances::reserved_balance(&0xD), 0);
			assert_eq!(Balances::free_balance(&0xD), 10000 - deposit);

			// cannot retract bid from auction that is in dispute
			assert_ok!(TaskAuction::dispute(Origin::signed(0xC), auction_key));
			assert_err!(
				TaskAuction::retract(Origin::signed(0xC), auction_key),
				Error::<Test>::AuctionDisputed
			);
		}
	})
}

#[test]
fn confirm() {
	new_test_ext().execute_with(|| {
		// non existing auction
		assert_err!(
			TaskAuction::confirm(Origin::signed(0xC), (0, 0)),
			Error::<Test>::AuctionKeyNotFound
		);
		// create an auction
		let deposit = 500;
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, deposit, 5, vec![0; 8]));
		if let AuctionEvent::Created { auction_key, bounty: _, terminal_block: _ } = last_event() {
			// only own of the auction can confirm
			assert_err!(
				TaskAuction::confirm(Origin::signed(0xC), auction_key),
				Error::<Test>::OwnerRequired
			);
			// can't confirm an auction with no bids
			assert_err!(
				TaskAuction::confirm(Origin::signed(0xA), auction_key),
				Error::<Test>::AuctionNotAssigned
			);
			// make a bid
			let pay = 900;
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, pay));
			assert_eq!(Balances::reserved_balance(&0xA), deposit + 1000);
			assert_eq!(Balances::reserved_balance(&0xC), deposit);
			// cannot confirm an auction that hasn't been assigned
			assert_err!(
				TaskAuction::confirm(Origin::signed(0xA), auction_key),
				Error::<Test>::AuctionNotAssigned
			);
			// wait until auction is assigned
			System::set_block_number(10);
			// expect success
			assert_ok!(TaskAuction::confirm(Origin::signed(0xA), auction_key));
			// check payements
			assert_eq!(Balances::reserved_balance(&0xA), 0);
			assert_eq!(Balances::reserved_balance(&0xC), 0);
			assert_eq!(Balances::free_balance(&0xA), 10000 - pay);
			assert_eq!(Balances::free_balance(&0xC), 10000 + pay);
			// auction should be deleted after transaction
			assert!(TaskAuction::auctions(auction_key).is_none());
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_none());
		}
	})
}

#[test]
fn cancel() {
	new_test_ext().execute_with(|| {
		// non existing auction
		assert_err!(
			TaskAuction::cancel(Origin::signed(0xC), (0, 0)),
			Error::<Test>::AuctionKeyNotFound
		);
		let deposit = 500;
		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, deposit, 5, vec![0; 8]));
		if let AuctionEvent::Created { auction_key, bounty: _, terminal_block: _ } = last_event() {
			// only own of the auction can cancel
			assert_err!(
				TaskAuction::cancel(Origin::signed(0xC), auction_key),
				Error::<Test>::OwnerRequired
			);
			// successful cancel with no bids
			assert_ok!(TaskAuction::cancel(Origin::signed(0xA), auction_key));
			assert_eq!(Balances::reserved_balance(&0xA), 0);
			assert_eq!(Balances::free_balance(&0xA), 10000);
			assert!(TaskAuction::auctions(auction_key).is_none());
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_none());
		}

		assert_ok!(TaskAuction::create(Origin::signed(0xA), 0xB, 1000, deposit, 5, vec![0; 8]));
		if let AuctionEvent::Created { auction_key, bounty: _, terminal_block: _ } = last_event() {
			assert_ok!(TaskAuction::bid(Origin::signed(0xC), auction_key, 800));
			assert_eq!(Balances::reserved_balance(&0xC), deposit);

			// cannot cancel auction that has been assigned
			System::set_block_number(10);
			assert_err!(
				TaskAuction::cancel(Origin::signed(0xA), auction_key),
				Error::<Test>::AuctionAssigned
			);
			System::set_block_number(1);

			// canceling auction with unassigned bids result in lost of deposit
			assert_ok!(TaskAuction::cancel(Origin::signed(0xA), auction_key));
			assert_eq!(Balances::reserved_balance(&0xA), 0);
			assert_eq!(Balances::reserved_balance(&0xC), 0);
			assert_eq!(Balances::free_balance(&0xA), 10000 - deposit);
			assert_eq!(Balances::free_balance(&0xC), 10000 + deposit);
			assert!(TaskAuction::auctions(auction_key).is_none());
			assert!(TaskAuction::bids(auction_key, (0, 0)).is_none());
		}
	})
}
