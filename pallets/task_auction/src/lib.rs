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
		sp_runtime::SaturatedConversion,
		traits::{Currency, ExistenceRequirement, ReservableCurrency},
	};

	type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
	type BalanceOf<T> = <<T as Config>::Currency as Currency<AccountIdOf<T>>>::Balance;
	type Key<T> = (AccountIdOf<T>, <T as frame_system::Config>::Index);

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Currency: ReservableCurrency<Self::AccountId>;

		#[pallet::constant] // put the constant in metadata
		type MinBounty: Get<BalanceOf<Self>>;
		#[pallet::constant] // put the constant in metadata
		type MinDeposit: Get<BalanceOf<Self>>;
		#[pallet::constant] // put the constant in metadata
		type MinBidRatio: Get<u32>;
		#[pallet::constant] // put the constant in metadata
		type MaxDataSize: Get<u32>;
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		AuctionIdNotFound,
		AuctionClosed,
		AuctionNotClosed,
		AuctionNotDisputed,

		MinBountyRequired,
		MinDepositRequired,
		MinBidRatioRequired,
		PreviousBidRequired,

		OwnerRequired,
		OwnerProhibited,
		ArbitratorProhibited,
		OriginIrrelevant,
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
		Confirmed { auction_key: Key<T>, confirmation: [Option<bool>; 2] },
		Cancelled { auction_key: Key<T> },
	}

	#[derive(Encode, Decode, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct Auction<T: Config> {
		pub arbitrator: T::AccountId,
		pub bounty: BalanceOf<T>,
		pub deposit: BalanceOf<T>,
		pub initial_block: T::BlockNumber,
		pub terminal_block: T::BlockNumber,
		pub data: BoundedVec<u8, T::MaxDataSize>,
		pub confirmation: [Option<bool>; 2],
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
			data: BoundedVec<u8, T::MaxDataSize>,
		) -> DispatchResult {
			// input checks
			let owner = ensure_signed(origin)?;
			let initial_block = frame_system::Pallet::<T>::block_number();
			ensure!(bounty >= T::MinBounty::get(), Error::<T>::MinBountyRequired);
			ensure!(deposit >= T::MinDeposit::get(), Error::<T>::MinDepositRequired);

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
				confirmation: [None; 2],
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
			// only owner of auction can extend
			let owner = ensure_signed(origin)?;
			ensure!(owner == auction_key.0, Error::<T>::OwnerRequired);
			// ensure auction is still open
			let mut auction =
				Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionIdNotFound)?;
			if let Some((_, price)) = Bids::<T>::get(&auction_key, Key::<T>::default()) {
				ensure!(price > auction.get_base_price(), Error::<T>::AuctionClosed);
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
		pub fn cancel(origin: OriginFor<T>, auction_key: Key<T>) -> DispatchResult {
			// only owner of auction can cancel
			let owner = ensure_signed(origin)?;
			ensure!(owner == auction_key.0, Error::<T>::OwnerRequired);

			// return bounty and deposit to owner
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionIdNotFound)?;
			T::Currency::unreserve(&owner, auction.bounty + auction.deposit);
			// if there are bids
			if let Some(((bidder, _), price)) = Bids::<T>::get(&auction_key, Key::<T>::default()) {
				// pay top bidder the full amount if auction is closed otherwise pay deposit
				let pay = if price > auction.get_base_price() { auction.deposit } else { price };
				T::Currency::transfer(&owner, &bidder, pay, ExistenceRequirement::AllowDeath)
					.expect("should have sufficient funds from unreserved bounty and deposit");
				// return deposit to top bidder
				T::Currency::unreserve(&bidder, auction.deposit);
			}
			// delete auction from storage
			Bids::<T>::remove_prefix(&auction_key, None);
			Auctions::<T>::remove(&auction_key);
			Self::deposit_event(Event::<T>::Cancelled { auction_key });
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
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionIdNotFound)?;
			ensure!(bidder != auction_key.0, Error::<T>::OwnerProhibited);
			ensure!(bidder != auction.arbitrator, Error::<T>::ArbitratorProhibited);

			// check if there is a previous bid
			let prev_bid = Bids::<T>::get(&auction_key, Key::<T>::default());
			let prev_key = if let Some((prev_key, prev_price)) = prev_bid {
				// ensure auction is still open
				ensure!(prev_price > auction.get_base_price(), Error::<T>::AuctionClosed);
				// ensure new bid is lower than prev bid
				ensure!(prev_price > price, Error::<T>::MinBidRatioRequired);
				// unreserve deposit of previous bidder
				T::Currency::unreserve(&prev_key.0, auction.deposit);
				prev_key
			} else {
				Key::<T>::default()
			};
			// all checks pass, reserve deposit of new bidder
			T::Currency::reserve(&bidder, auction.deposit)?;
			// insert new bid
			let nonce = frame_system::Pallet::<T>::account_nonce(&bidder);
			let bid_key = (bidder, nonce);
			Bids::<T>::insert(&auction_key, &bid_key, (prev_key, price));
			Bids::<T>::insert(&auction_key, Key::<T>::default(), (bid_key.clone(), price));

			Self::deposit_event(Event::<T>::Bid { auction_key, bid_key, price });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn retract(origin: OriginFor<T>, auction_key: Key<T>) -> DispatchResult {
			let bidder = ensure_signed(origin)?;
			let auction = Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionIdNotFound)?;
			let (prev_key, prev_price) = Bids::<T>::get(&auction_key, Key::<T>::default())
				.ok_or(Error::<T>::PreviousBidRequired)?;
			ensure!(bidder == prev_key.0, Error::<T>::PreviousBidRequired);
			/*
			let pay =

			let prev_key = if let Some((prev_key, prev_price)) = prev_bid {
				if ()
				// ensure auction is still open
				ensure!(prev_price > auction.get_base_price(), Error::<T>::AuctionClosed);
				// ensure new bid is lower than prev bid
				ensure!(prev_price > price, Error::<T>::MinBidRatioRequired);
				// unreserve deposit of previous bidder
				T::Currency::unreserve(&prev_key.0, auction.deposit);
				prev_key
			} else {
				Key::<T>::default()
			};
			*/
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn confirm(
			origin: OriginFor<T>,
			auction_key: Key<T>,
			fulfilled: bool,
		) -> DispatchResult {
			let signer = ensure_signed(origin)?;
			let owner = &auction_key.0;
			// ensure auction is closed
			let mut auction =
				Auctions::<T>::get(&auction_key).ok_or(Error::<T>::AuctionIdNotFound)?;
			if let Some(((bidder, _), price)) = Bids::<T>::get(&auction_key, Key::<T>::default()) {
				ensure!(price <= auction.get_base_price(), Error::<T>::AuctionNotClosed);
				if signer == auction.arbitrator {
					/*
					if if let [Some(bidder_confirm), Some(owner_confirm)] = auction.confirmation {
						bidder_confirm != owner_confirm
					} else {
						false
					} {} else {Error::<T>}
					*/
					match auction.confirmation {
						[Some(bidder_confirm), Some(owner_confirm)]
							if bidder_confirm != owner_confirm =>
						{
							T::Currency::unreserve(owner, auction.deposit + auction.bounty);
							T::Currency::unreserve(&bidder, auction.deposit);
							// arbitrator takes deposit from losing party of dispute
							T::Currency::transfer(
								owner,
								if fulfilled { owner } else { &bidder },
								auction.deposit,
								ExistenceRequirement::AllowDeath,
							)
							.unwrap();
							// owner pays bidder if task has been fulfilled
							if fulfilled {
								T::Currency::transfer(
									owner,
									&bidder,
									price,
									ExistenceRequirement::AllowDeath,
								)
								.unwrap();
							}
							Ok(())
						},
						_ => Err(Error::<T>::AuctionNotDisputed),
					}?;
				} else if signer == bidder {
				} else if signer == *owner {
					if let Some(bidder_confirm) = auction.confirmation[0] {
					} else {
						auction.confirmation[1] = Some(fulfilled);
						let confirmation = auction.confirmation.clone();
						Auctions::<T>::insert(&auction_key, auction);
						Self::deposit_event(Event::<T>::Confirmed { auction_key, confirmation });
					}
				} else {
					Err(Error::<T>::OriginIrrelevant)?;
				}
			}
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
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);
}
