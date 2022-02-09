#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	use frame_support::{
		inherent::Vec,
		sp_runtime::SaturatedConversion,
		traits::{Currency, ExistenceRequirement, ReservableCurrency},
	};

	type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
	type BalanceOf<T> = <<T as Config>::Currency as Currency<AccountIdOf<T>>>::Balance;
	type Key<T> = (AccountIdOf<T>, <T as frame_system::Config>::Index);

	// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Currency: ReservableCurrency<Self::AccountId>;

		#[pallet::constant]
		type MinBounty: Get<BalanceOf<Self>>;
		#[pallet::constant]
		type MinDeposit: Get<BalanceOf<Self>>;
		#[pallet::constant]
		type MinBidRatio: Get<u8>;
		#[pallet::constant]
		type MaxDataSize: Get<u32>;
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		AuctionKeyNotFound,
		AuctionAssigned,
		AuctionNotAssigned,
		AuctionDisputed,
		AuctionNotDisputed,

		MinBountyRequired,
		MinDepositRequired,
		MinBidRatioRequired,
		MaxDataSizeExceeded,

		TopBidRequired,
		OwnerRequired,
		OriginProhibited,
	}

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		Created { auction_key: Key<T>, bounty: BalanceOf<T>, terminal_block: T::BlockNumber },
		Extended { auction_key: Key<T>, bounty: BalanceOf<T>, terminal_block: T::BlockNumber },

		Bid { auction_key: Key<T>, bid_key: Key<T>, price: BalanceOf<T> },
		Retracted { auction_key: Key<T>, bid_key: Key<T>, price: BalanceOf<T> },

		Confirmed { auction_key: Key<T> },
		Cancelled { auction_key: Key<T> },

		Disputed { auction_key: Key<T> },
		Arbitrated { auction_key: Key<T>, fulfilled: bool },
	}

	// Pallets types to use in dispatchable interface.
	#[derive(Encode, Decode, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct Auction<T: Config> {
		pub arbitrator: T::AccountId,
		pub bounty: BalanceOf<T>,
		pub deposit: BalanceOf<T>,
		pub initial_block: T::BlockNumber,
		pub terminal_block: T::BlockNumber,
		pub data: Vec<u8>,
		pub in_dispute: bool,
	}

	// The pallet's runtime storage items.
	// https://docs.substrate.io/v3/runtime/storage
	#[pallet::storage]
	#[pallet::getter(fn auctions)]
	pub(super) type Auctions<T: Config> =
		StorageMap<_, Twox64Concat, Key<T>, Auction<T>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn bids)]
	pub(super) type Bids<T: Config> = StorageDoubleMap<
		_,
		Twox64Concat,
		Key<T>,
		Twox64Concat,
		Key<T>,
		(Key<T>, BalanceOf<T>),
		OptionQuery,
	>;

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn create(
			origin: OriginFor<T>,
			arbitrator: T::AccountId,
			bounty: BalanceOf<T>,
			deposit: BalanceOf<T>,
			terminal_block: T::BlockNumber,
			data: Vec<u8>,
		) -> DispatchResult {
			// input checks
			let owner = ensure_signed(origin)?;
			let initial_block = frame_system::Pallet::<T>::block_number();
			ensure!(bounty >= T::MinBounty::get(), Error::<T>::MinBountyRequired);
			ensure!(deposit >= T::MinDeposit::get(), Error::<T>::MinDepositRequired);
			ensure!(
				data.len() <= T::MaxDataSize::get().try_into().unwrap(),
				Error::<T>::MaxDataSizeExceeded
			);

			// reserve balance for bounty and deposit
			T::Currency::reserve(&owner, bounty + deposit)?;

			// generate auction key
			let nonce = frame_system::Pallet::<T>::account_nonce(&owner);
			let auction_key = (owner, nonce);

			// create and insert new auction
			let auction = Auction::<T> {
				arbitrator,
				bounty,
				deposit,
				initial_block,
				terminal_block,
				data,
				in_dispute: false,
			};
			Auctions::<T>::insert(&auction_key, auction);

			Self::deposit_event(Event::<T>::Created { auction_key, bounty, terminal_block });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn extend(
			origin: OriginFor<T>,
			auction_key: Key<T>,
			bounty: BalanceOf<T>,
			terminal_block: T::BlockNumber,
		) -> DispatchResult {
			let owner = ensure_signed(origin)?;
			let mut auction =
				Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionKeyNotFound)?;
			// only owner of auction can extend
			ensure!(owner == auction_key.0, Error::<T>::OwnerRequired);
			// ensure auction is not assigned
			if let Some((_, price)) = Bids::<T>::get(&auction_key, Key::<T>::default()) {
				ensure!(!auction.is_assigned(price), Error::<T>::AuctionAssigned);
			}
			// reserve the difference in bounty
			ensure!(bounty > auction.bounty, Error::<T>::MinBountyRequired);
			T::Currency::reserve(&owner, bounty - auction.bounty)?;
			// update auction
			auction.bounty = bounty;
			auction.terminal_block = terminal_block;
			Auctions::<T>::insert(&auction_key, auction);

			Self::deposit_event(Event::<T>::Extended { auction_key, bounty, terminal_block });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn bid(
			origin: OriginFor<T>,
			auction_key: Key<T>,
			price: BalanceOf<T>,
		) -> DispatchResult {
			// input checks
			let bidder = ensure_signed(origin)?;
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionKeyNotFound)?;
			ensure!(bidder != auction_key.0, Error::<T>::OriginProhibited);
			ensure!(bidder != auction.arbitrator, Error::<T>::OriginProhibited);

			// check if there is a previous bid
			let prev_bid = Bids::<T>::get(&auction_key, Key::<T>::default());
			let prev_key = if let Some((prev_key, prev_price)) = prev_bid {
				// ensure auction is not assigned
				ensure!(!auction.is_assigned(prev_price), Error::<T>::AuctionAssigned);
				// ensure new bid is lower than prev bid
				ensure!(
					prev_price * T::MinBidRatio::get().into() > price * 255u8.into(),
					Error::<T>::MinBidRatioRequired
				);
				// unreserve deposit of previous bidder
				T::Currency::unreserve(&prev_key.0, auction.deposit);
				prev_key
			} else {
				// first bid must be within bounty
				ensure!(auction.bounty >= price, Error::<T>::MinBidRatioRequired);
				Key::<T>::default()
			};
			// all checks pass, reserve deposit of new bidder
			T::Currency::reserve(&bidder, auction.deposit)?;
			// insert new bid
			let bid_key = (bidder, prev_key.1 + 1u8.into());
			Bids::<T>::insert(&auction_key, &bid_key, (prev_key, price));
			Bids::<T>::insert(&auction_key, Key::<T>::default(), (bid_key.clone(), price));

			Self::deposit_event(Event::<T>::Bid { auction_key, bid_key, price });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn retract(origin: OriginFor<T>, auction_key: Key<T>) -> DispatchResult {
			let bidder = ensure_signed(origin)?;
			// fetch auction and previous bid
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionKeyNotFound)?;
			let (mut top_key, top_price) = Bids::<T>::get(&auction_key, Key::<T>::default())
				.ok_or(Error::<T>::TopBidRequired)?;
			// only the top bid can be retracted
			ensure!(bidder == top_key.0, Error::<T>::TopBidRequired);
			// cannot retract bid when auction is in dispute
			ensure!(!auction.in_dispute, Error::<T>::AuctionDisputed);
			// bidder loses deposit to owner if auction is assigned
			T::Currency::unreserve(&bidder, auction.deposit);
			if auction.is_assigned(top_price) {
				T::Currency::transfer(
					&bidder,
					&auction_key.0,
					auction.deposit,
					ExistenceRequirement::AllowDeath,
				)
				.unwrap();
			}

			let (bid_key, price) = loop {
				// remove top bid
				let (prev_key, _) = Bids::<T>::take(&auction_key, &top_key).unwrap();
				// if there is no previous bid, reset bid vector
				if prev_key == Key::<T>::default() {
					Bids::<T>::remove_prefix(&auction_key, None);
					break (prev_key, auction.bounty)
				}
				// use previous bid as top bid if funds can be reserved
				else if T::Currency::reserve(&prev_key.0, auction.deposit).is_ok() {
					let (_, prev_price) = Bids::<T>::get(&auction_key, &prev_key).unwrap();
					Bids::<T>::insert(
						&auction_key,
						Key::<T>::default(),
						(prev_key.clone(), prev_price),
					);
					break (prev_key, prev_price)
				}
				// otherwise continue down the stack
				top_key = prev_key;
			};

			Self::deposit_event(Event::<T>::Retracted { auction_key, bid_key, price });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn confirm(origin: OriginFor<T>, auction_key: Key<T>) -> DispatchResult {
			let owner = ensure_signed(origin)?;
			// fetch auction and top bid
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionKeyNotFound)?;
			// only owner of auction can confirm
			ensure!(owner == auction_key.0, Error::<T>::OwnerRequired);
			if let Some(((bidder, _), price)) = Bids::<T>::get(&auction_key, Key::<T>::default()) {
				// only assigned auctions can be confirmed
				ensure!(auction.is_assigned(price), Error::<T>::AuctionNotAssigned);
				// unreserve deposits of bidder and owner
				T::Currency::unreserve(&bidder, auction.deposit);
				T::Currency::unreserve(&owner, auction.deposit + auction.bounty);
				// owner pays bidder the agreed price
				T::Currency::transfer(&owner, &bidder, price, ExistenceRequirement::AllowDeath)
					.unwrap();
			} else {
				Err(Error::<T>::AuctionNotAssigned)?;
			}
			// delete auction from storage
			Bids::<T>::remove_prefix(&auction_key, None);
			Auctions::<T>::remove(&auction_key);
			Self::deposit_event(Event::<T>::Confirmed { auction_key });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn cancel(origin: OriginFor<T>, auction_key: Key<T>) -> DispatchResult {
			let owner = ensure_signed(origin)?;
			// fetch auction and top bid
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionKeyNotFound)?;
			// only owner of auction can cancel
			ensure!(owner == auction_key.0, Error::<T>::OwnerRequired);
			if let Some(((bidder, _), price)) = Bids::<T>::get(&auction_key, Key::<T>::default()) {
				// only unassigned auctions can be cancelled
				ensure!(!auction.is_assigned(price), Error::<T>::AuctionAssigned);
				// unreserve deposits of bidder and owner
				T::Currency::unreserve(&bidder, auction.deposit);
				T::Currency::unreserve(&owner, auction.deposit + auction.bounty);
				// owner pays bidder the deposit
				T::Currency::transfer(
					&owner,
					&bidder,
					auction.deposit,
					ExistenceRequirement::AllowDeath,
				)
				.unwrap();
			} else {
				// unreserve deposits of owner
				T::Currency::unreserve(&owner, auction.deposit + auction.bounty);
			}
			// delete auction from storage
			Bids::<T>::remove_prefix(&auction_key, None);
			Auctions::<T>::remove(&auction_key);
			Self::deposit_event(Event::<T>::Cancelled { auction_key });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn dispute(origin: OriginFor<T>, auction_key: Key<T>) -> DispatchResult {
			let origin = ensure_signed(origin)?;
			// fetch auction
			let mut auction =
				Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionKeyNotFound)?;
			// auction is already in dispute
			ensure!(!auction.in_dispute, Error::<T>::AuctionDisputed);
			// fetch top bid
			if let Some(((bidder, _), price)) = Bids::<T>::get(&auction_key, Key::<T>::default()) {
				// only assigned auctions can be disputed
				ensure!(auction.is_assigned(price), Error::<T>::AuctionNotAssigned);
				// only owner or bidder can dispute
				ensure!(origin == bidder || origin == auction_key.0, Error::<T>::OriginProhibited);
			} else {
				Err(Error::<T>::AuctionNotAssigned)?
			}
			auction.in_dispute = true;
			Auctions::<T>::insert(&auction_key, auction);
			Self::deposit_event(Event::<T>::Disputed { auction_key });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn arbitrate(
			origin: OriginFor<T>,
			auction_key: Key<T>,
			fulfilled: bool,
		) -> DispatchResult {
			let arbitrator = ensure_signed(origin)?;
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionKeyNotFound)?;
			// only the arbitrator is allowed
			ensure!(arbitrator == auction.arbitrator, Error::<T>::OriginProhibited);
			// auction must be in dispute
			ensure!(auction.in_dispute, Error::<T>::AuctionDisputed);
			// fetch bidder
			let ((bidder, _), price) = Bids::<T>::get(&auction_key, Key::<T>::default()).unwrap();
			// unreserve funds
			T::Currency::unreserve(&auction_key.0, auction.deposit + auction.bounty);
			T::Currency::unreserve(&bidder, auction.deposit);
			// pay bidder if task is fulfilled
			let loser = if fulfilled {
				T::Currency::transfer(
					&auction_key.0,
					&bidder,
					price,
					ExistenceRequirement::AllowDeath,
				)
				.unwrap();
				&auction_key.0
			} else {
				&bidder
			};
			// losing side pays arbitrator their deposit
			T::Currency::transfer(
				loser,
				&arbitrator,
				auction.deposit,
				ExistenceRequirement::AllowDeath,
			)
			.unwrap();
			// delete auction from storage
			Bids::<T>::remove_prefix(&auction_key, None);
			Auctions::<T>::remove(&auction_key);
			Self::deposit_event(Event::<T>::Arbitrated { auction_key, fulfilled });
			Ok(())
		}
	}

	// helper functions
	impl<T: Config> Auction<T> {
		pub fn get_base_price(&self) -> BalanceOf<T> {
			let now = frame_system::Pallet::<T>::block_number();
			if now < self.terminal_block {
				self.bounty * (now - self.initial_block).saturated_into::<u32>().into() /
					(self.terminal_block - self.initial_block).saturated_into::<u32>().into()
			} else {
				self.bounty
			}
		}

		pub fn is_assigned(&self, top_bid: BalanceOf<T>) -> bool {
			top_bid <= self.get_base_price()
		}
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);
}
