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

	use frame_support::traits::{
		Currency, ExistenceRequirement, ReservableCurrency, WithdrawReasons,
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

	#[derive(Encode, Decode, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct Auction<T: Config> {
		pub arbitrator: T::AccountId,
		pub bounty: BalanceOf<T>,
		pub deposit: BalanceOf<T>,
		pub deadline: T::BlockNumber,
		pub data: BoundedVec<u8, T::MaxDataSize>,
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

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		Created { auction_key: Key<T>, bounty: BalanceOf<T>, deadline: T::BlockNumber },
		Bid { auction_key: Key<T>, bid_key: Key<T>, price: BalanceOf<T> },
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		DeadlineExpired,
		MinBountyRequired,
		MinDepositRequired,
		MinBidRatioRequired,
		AuctionIdNotFound,

		BidderIsEmployer,
		BidderIsArbitrator,
	}
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
			deadline: T::BlockNumber,
			data: BoundedVec<u8, T::MaxDataSize>,
		) -> DispatchResult {
			// input checks
			let employer = ensure_signed(origin)?;
			ensure!(bounty >= T::MinBounty::get(), Error::<T>::MinBountyRequired);
			ensure!(deposit >= T::MinDeposit::get(), Error::<T>::MinDepositRequired);
			ensure!(
				deadline > frame_system::Pallet::<T>::block_number(),
				Error::<T>::DeadlineExpired
			);

			// reserve balance for bounty and deposit
			T::Currency::reserve(&employer, bounty + deposit)?;

			// generate auction key
			let nonce = frame_system::Pallet::<T>::account_nonce(&employer);
			let auction_key = (employer, nonce);

			// create and insert new auction
			let auction = Auction::<T> { arbitrator, bounty, deposit, deadline, data };
			Auctions::<T>::insert(&auction_key, auction);

			Self::deposit_event(Event::<T>::Created { auction_key, bounty, deadline });
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
			ensure!(
				auction.deadline >= frame_system::Pallet::<T>::block_number(),
				Error::<T>::DeadlineExpired
			);
			ensure!(bidder != auction_key.0, Error::<T>::BidderIsEmployer);
			ensure!(bidder != auction.arbitrator, Error::<T>::BidderIsArbitrator);

			// if there is a previous bid, ensure new bid is lower,
			// then unreserve deposit of previous bidder
			let prev_bid = Bids::<T>::get(&auction_key, Key::<T>::default());
			let prev_key = if let Some((prev_key, prev_price)) = prev_bid {
				ensure!(prev_price > price, Error::<T>::MinBidRatioRequired);
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
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);
}
